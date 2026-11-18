//! Outbound SMTP for auth emails. Without SMTP_HOST configured, sending fails
//! with a clear error so the client can tell the user mail isn't set up.

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{MultiPart, SinglePart},
    transport::smtp::authentication::Credentials,
    transport::smtp::client::{Tls, TlsParameters},
};

use crate::{
    auth::AppState,
    error::{ApiError, ApiResult},
    store,
};

fn render(input: &str, vars: &[(&str, &str)], html: bool) -> String {
    let mut out = input.to_string();
    for (key, value) in vars {
        let value = if html {
            value
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&#39;")
        } else {
            (*value).to_string()
        };
        out = out.replace(key, &value);
    }
    out
}

/// Appwrite/Semester templates use their own variable names; accept both.
fn with_aliases<'a>(vars: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut all = vars.to_vec();
    for (key, value) in vars {
        let alias = match *key {
            "{{link}}" => Some("{{redirect}}"),
            "{{code}}" => Some("{{otp}}"),
            "{{email}}" => Some("{{user}}"),
            "{{appName}}" => Some("{{project}}"),
            _ => None,
        };
        if let Some(alias) = alias {
            all.push((alias, value));
        }
    }
    all
}

/// Render the named stored template, falling back to built-in defaults if the
/// row was removed, then send it as multipart/alternative when HTML is present.
pub async fn from_template(
    state: &AppState,
    kind: &str,
    to: &str,
    vars: &[(&str, &str)],
    fallback: (&str, &str, &str),
) -> ApiResult<()> {
    let template = store::get_mail_template(&state.writer().await, kind).await?;
    let (subject, text, html) = match template {
        Some(t) => (t.subject, t.body, t.html_body),
        None => (
            fallback.0.to_string(),
            fallback.1.to_string(),
            fallback.2.to_string(),
        ),
    };
    let all = with_aliases(vars);
    send(
        state,
        to,
        &render(&subject, &all, false),
        &render(&text, &all, false),
        &render(&html, &all, true),
    )
    .await
}

pub async fn send(
    state: &AppState,
    to: &str,
    subject: &str,
    text_body: &str,
    html_body: &str,
) -> ApiResult<()> {
    let cfg = state.cfg();
    if cfg.smtp_host.is_empty() {
        return Err(ApiError::bad_request("SMTP is not configured"));
    }
    let from = cfg
        .smtp_from
        .parse()
        .map_err(|_| ApiError::bad_request("SMTP_FROM is not a valid email address"))?;
    let to = to
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid recipient email"))?;
    let builder = Message::builder().from(from).to(to).subject(subject);
    let email = if html_body.trim().is_empty() {
        builder.body(text_body.to_string())
    } else {
        builder.multipart(
            MultiPart::alternative()
                .singlepart(SinglePart::plain(text_body.to_string()))
                .singlepart(SinglePart::html(html_body.to_string())),
        )
    }
    .map_err(ApiError::internal)?;

    let tls = TlsParameters::new(cfg.smtp_host.clone()).map_err(ApiError::internal)?;
    let mut transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.smtp_host)
        .port(cfg.smtp_port)
        .tls(if cfg.smtp_tls {
            Tls::Wrapper(tls)
        } else {
            Tls::Required(tls)
        });
    if !cfg.smtp_user.is_empty() {
        transport = transport.credentials(Credentials::new(
            cfg.smtp_user.clone(),
            cfg.smtp_password.clone(),
        ));
    }
    let mailer: AsyncSmtpTransport<Tokio1Executor> = transport.build();
    mailer.send(email).await.map_err(|err| {
        tracing::warn!("SMTP send failed: {err}");
        ApiError::bad_request(format!("SMTP send failed: {err}"))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn placeholders_render_in_both_parts() {
        let vars = [("{{code}}", "123456"), ("{{email}}", "a@b.test")];
        assert_eq!(
            render("Code {{code}} for {{email}}", &vars, false),
            "Code 123456 for a@b.test"
        );
        assert_eq!(
            render("<b>{{code}}</b>", &vars, true),
            "<b>123456</b>"
        );
    }

    #[test]
    fn semester_variables_are_aliased() {
        let vars = [("{{link}}", "https://x.test/r"), ("{{code}}", "123456")];
        let all = super::with_aliases(&vars);
        assert_eq!(render("<a href=\"{{redirect}}\">{{otp}}</a>", &all, true), "<a href=\"https://x.test/r\">123456</a>");
    }

    #[test]
    fn html_values_are_escaped() {
        let vars = [("{{link}}", "https://x.test/?a=1&b=2")];
        assert_eq!(
            render("<a href=\"{{link}}\">x</a>", &vars, true),
            "<a href=\"https://x.test/?a=1&amp;b=2\">x</a>"
        );
        assert_eq!(
            render("{{link}}", &vars, false),
            "https://x.test/?a=1&b=2"
        );
    }
}

"use client";

import { useEffect, useState } from "react";

import { api } from "@/lib/api";
import { useApi, useToast } from "@/lib/hooks";
import {
  Badge,
  Field,
  Icon,
  Loading,
  Notice,
  PageHead,
  Toast,
} from "@/components/ui";

const TEMPLATES = [
  {
    kind: "recovery",
    label: "Password reset",
    hint: "Sent when someone requests a reset link. Include {{link}}.",
  },
  {
    kind: "verification",
    label: "Email verification",
    hint: "Sent after sign-up. Include {{link}}.",
  },
  {
    kind: "mfa",
    label: "Sign-in code",
    hint: "Sent during email two-factor sign-in. Include {{code}}.",
  },
];

export default function MailPage() {
  const { data, loading, error } = useApi("/mail");
  const {
    data: templateData,
    loading: templatesLoading,
    error: templatesError,
    reload: reloadTemplates,
  } = useApi("/mail/templates");
  const { toast, notify } = useToast();

  const [kind, setKind] = useState("recovery");
  const [form, setForm] = useState({ subject: "", body: "", html: "" });
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState("");

  useEffect(() => {
    const current = templateData?.templates?.find((t) => t.kind === kind);
    if (current)
      setForm({ subject: current.subject, body: current.body, html: current.html || "" });
  }, [templateData, kind]);

  async function save(event) {
    event.preventDefault();
    setBusy(true);
    setSaveError("");
    try {
      await api("/mail/templates", { method: "PUT", body: { kind, ...form } });
      reloadTemplates();
      notify("Template saved", "ok");
    } catch (err) {
      setSaveError(err.message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <PageHead
        eyebrow="Infrastructure"
        title="Mail"
        description="SMTP settings and auth email templates used for password resets, verification and two-factor sign-in codes. SMTP is read from backend/.env; templates are stored here on the server."
        actions={
          data ? (
            <Badge tone={data.configured ? "ok" : "danger"}>
              {data.configured ? "configured" : "not configured"}
            </Badge>
          ) : null
        }
      />

      {error ? <Notice tone="danger">{error.message}</Notice> : null}

      {loading ? <Loading /> : data ? (
        <>
          <Notice tone={data.configured ? "ok" : "warn"}>
            {data.configured
              ? `Email is enabled. Outbound mail is sent as ${data.from || data.user || data.host}.`
              : "Email is disabled: set SMTP_HOST (and usually SMTP_USER, SMTP_PASSWORD, SMTP_FROM) in backend/.env, then restart the API."}
          </Notice>

          <section className="panel">
            <div className="panel-head">
              <h2>SMTP settings</h2>
              <span className="mono">{data.host || "no host"}</span>
            </div>
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>Setting</th>
                    <th>Value</th>
                  </tr>
                </thead>
                <tbody>
                  <tr>
                    <td>SMTP host</td>
                    <td className="mono">{data.host || "—"}</td>
                  </tr>
                  <tr>
                    <td>Port</td>
                    <td className="mono">{data.port ?? "—"}</td>
                  </tr>
                  <tr>
                    <td>Username</td>
                    <td className="mono">{data.user || "—"}</td>
                  </tr>
                  <tr>
                    <td>From address</td>
                    <td className="mono">{data.from || "—"}</td>
                  </tr>
                  <tr>
                    <td>Connection</td>
                    <td>{data.tls === "implicit" ? "Implicit TLS (port 465)" : "STARTTLS"}</td>
                  </tr>
                  <tr>
                    <td>Password</td>
                    <td>
                      {data.passwordSet ? (
                        <Badge tone="ok">set · hidden</Badge>
                      ) : (
                        <Badge tone="danger">not set</Badge>
                      )}
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </section>

          <section className="panel" style={{ marginTop: 18 }}>
            <div className="panel-head">
              <h2>Email templates</h2>
              <span className="small subtle">plain text + HTML</span>
            </div>

            {templatesError ? <Notice tone="danger">{templatesError.message}</Notice> : null}
            {templatesLoading ? <Loading /> : (
              <>
                <div className="btn-row" style={{ padding: "16px 20px 0" }}>
                  {TEMPLATES.map((item) => (
                    <button
                      key={item.kind}
                      type="button"
                      className={`btn btn-sm ${kind === item.kind ? "btn-primary" : ""}`}
                      onClick={() => {
                        setKind(item.kind);
                        setSaveError("");
                      }}
                    >
                      {item.label}
                    </button>
                  ))}
                </div>

                <form onSubmit={save} style={{ padding: "16px 20px 20px" }}>
                  <p className="small subtle">
                    {TEMPLATES.find((item) => item.kind === kind)?.hint}
                    {" "}Available placeholders: <code className="mono">{"{{link}}"}</code>,{" "}
                    <code className="mono">{"{{code}}"}</code>,{" "}
                    <code className="mono">{"{{email}}"}</code>,{" "}
                    <code className="mono">{"{{appName}}"}</code>.
                  </p>

                  <Field label="Subject" htmlFor="template-subject">
                    <input
                      id="template-subject"
                      className="input"
                      value={form.subject}
                      onChange={(event) =>
                        setForm((current) => ({ ...current, subject: event.target.value }))
                      }
                      required
                    />
                  </Field>

                  <Field
                    label="Plain-text body"
                    hint="Always included — mail clients that ignore HTML show this."
                    htmlFor="template-body"
                  >
                    <textarea
                      id="template-body"
                      className="textarea"
                      rows={7}
                      value={form.body}
                      onChange={(event) =>
                        setForm((current) => ({ ...current, body: event.target.value }))
                      }
                      required
                    />
                  </Field>

                  <Field
                    label="HTML body"
                    hint="Leave empty to send text only. Placeholders are HTML-escaped when substituted."
                    htmlFor="template-html"
                  >
                    <textarea
                      id="template-html"
                      className="textarea mono"
                      rows={7}
                      value={form.html}
                      onChange={(event) =>
                        setForm((current) => ({ ...current, html: event.target.value }))
                      }
                    />
                  </Field>

                  {saveError ? <Notice tone="danger">{saveError}</Notice> : null}

                  <div className="btn-row">
                    <button className="btn btn-primary" disabled={busy} type="submit">
                      <Icon name="check" size={14} />
                      {busy ? "Saving…" : "Save template"}
                    </button>
                  </div>
                </form>
              </>
            )}
          </section>

          <p className="small subtle" style={{ marginTop: 16 }}>
            <Icon name="info" size={13} /> SMTP settings come from the server environment; the
            password is never returned by the API. Templates are saved on the server and take
            effect on the next email.
          </p>
        </>
      ) : null}

      <Toast message={toast?.message} tone={toast?.tone} />
    </>
  );
}

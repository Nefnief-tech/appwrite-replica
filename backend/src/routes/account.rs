use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
};
use axum_extra::extract::CookieJar;
use chrono::Utc;
use rand::Rng;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::{
        ApiCtx, AppState, build_cookie, create_user_session, destroy_all_user_sessions,
        destroy_other_user_sessions, destroy_user_session, find_session_by_id, no_store_cookie,
        rate_limit, session_cookie_name, set_session_mfa_pending,
    },
    crypto::{generate_token, sha256_hex},
    error::{ApiError, ApiResult},
    mailer,
    models::{UserRow, mfa_enabled, mfa_recovery_codes, session_json, user_json},
    store,
};

#[derive(Deserialize)]
pub struct CreateAccountBody {
    #[serde(rename = "userId")]
    pub user_id: Option<String>,
    pub email: String,
    pub password: String,
    pub name: Option<String>,
}

pub async fn create_account(
    State(state): State<AppState>,
    ctx: ApiCtx,
    jar: CookieJar,
    Json(body): Json<CreateAccountBody>,
) -> ApiResult<(StatusCode, CookieJar, Json<Value>)> {
    let user = store::create_user(
        &state.writer().await,
        &ctx.project.id,
        body.user_id,
        &body.email,
        body.name.as_deref().unwrap_or(""),
        Some(body.password),
        false,
    )
    .await?;
    let (token, _) = create_user_session(&state, &ctx.project.id, &user.id, &user.email).await?;
    let cookie = build_cookie(
        state.cfg(),
        session_cookie_name(&ctx.project.id),
        token,
        365,
    );
    Ok((StatusCode::CREATED, jar.add(cookie), Json(user_json(&user))))
}

#[derive(Deserialize)]
pub struct LoginBody {
    pub email: String,
    pub password: String,
    #[allow(dead_code)]
    pub duration: Option<i64>,
}

pub async fn login(
    State(state): State<AppState>,
    ctx: ApiCtx,
    jar: CookieJar,
    Json(body): Json<LoginBody>,
) -> ApiResult<(StatusCode, CookieJar, Json<Value>)> {
    let email = store::normalize_email(&body.email)?;
    let limit_key = format!(
        "login:{}:{}",
        ctx.project.id,
        crate::crypto::sha256_hex(&email)
    );
    rate_limit(&state, &limit_key, 10).await?;

    let pool = state.writer().await;
    let user = store::find_user_by_email(&pool, &ctx.project.id, &email)
        .await?
        .ok_or_else(|| {
            ApiError::unauthorized(
                "Invalid credentials. If this account was imported, an admin must set a password.",
            )
        })?;
    if !user.status {
        return Err(ApiError::unauthorized("This account is disabled"));
    }
    let Some(hash) = user.password_hash.as_ref() else {
        return Err(ApiError::unauthorized(
            "Invalid credentials. If this account was imported, an admin must set a password.",
        ));
    };
    let valid = crate::crypto::verify_password(body.password, hash.clone())
        .await
        .map_err(ApiError::internal)?;
    if !valid {
        return Err(ApiError::unauthorized("Invalid credentials"));
    }
    crate::auth::clear_rate_limit(&state, &limit_key).await?;
    let (token, session) =
        create_user_session(&state, &ctx.project.id, &user.id, &user.email).await?;
    if mfa_enabled(&user) {
        set_session_mfa_pending(&state, &ctx.project.id, &token, true).await?;
    }
    let cookie = build_cookie(
        state.cfg(),
        session_cookie_name(&ctx.project.id),
        token.clone(),
        365,
    );
    let mut payload = session_json(&session, true);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("secret".into(), json!(token));
    }
    Ok((StatusCode::CREATED, jar.add(cookie), Json(payload)))
}

pub async fn get_account(ctx: ApiCtx) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    Ok(Json(user_json(user)))
}

#[derive(Deserialize)]
pub struct NameBody {
    pub name: String,
}

pub async fn update_name(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Json(body): Json<NameBody>,
) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    if body.name.trim().is_empty() || body.name.chars().count() > 128 {
        return Err(ApiError::bad_request("Name must be between 1 and 128 characters"));
    }
    let updated = sqlx::query_as::<_, UserRow>(
        "update ow_users set name = $3, updated_at = now() where project_id = $1 and id = $2 returning id, project_id, email, name, password_hash, prefs, status, imported, email_verified, registration, password_update, created_at, updated_at",
    )
    .bind(&ctx.project.id)
    .bind(&user.id)
    .bind(body.name.trim())
    .fetch_one(&state.writer().await)
    .await?;
    Ok(Json(user_json(&updated)))
}

#[derive(Deserialize)]
pub struct PasswordBody {
    pub password: String,
    #[serde(rename = "oldPassword")]
    pub old_password: Option<String>,
}

pub async fn update_password(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Json(body): Json<PasswordBody>,
) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    if let Some(hash) = user.password_hash.as_ref() {
        let old = body
            .old_password
            .clone()
            .ok_or_else(|| ApiError::bad_request("oldPassword is required"))?;
        let valid = crate::crypto::verify_password(old, hash.clone())
            .await
            .map_err(ApiError::internal)?;
        if !valid {
            return Err(ApiError::unauthorized("Current password is incorrect"));
        }
    }
    let updated = store::set_user_password(
        &state.writer().await,
        &ctx.project.id,
        &user.id,
        &body.password,
    )
    .await?;
    if let Some(token) = &ctx.session_token {
        destroy_other_user_sessions(&state, &ctx.project.id, &user.id, token).await?;
    }
    Ok(Json(user_json(&updated)))
}

#[derive(Deserialize)]
pub struct PrefsBody {
    pub prefs: Value,
}

pub async fn get_prefs(ctx: ApiCtx) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    Ok(Json(user.prefs.clone()))
}

pub async fn update_prefs(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Json(body): Json<PrefsBody>,
) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    // Appwrite replaces preferences with the supplied object.
    let mut prefs = match body.prefs {
        Value::Object(_) => body.prefs,
        _ => return Err(ApiError::bad_request("prefs must be an object")),
    };
    // MFA state is server-managed: keep it even when a client replaces prefs
    if let Some(obj) = prefs.as_object_mut() {
        for key in ["__mfa", "__mfaRecovery"] {
            if let Some(value) = user.prefs.get(key) {
                obj.insert(key.to_string(), value.clone());
            }
        }
    }
    let updated = store::update_user_prefs(
        &state.writer().await,
        &ctx.project.id,
        &user.id,
        prefs,
    )
    .await?;
    Ok(Json(user_json(&updated)))
}

pub async fn list_sessions(ctx: ApiCtx) -> ApiResult<Json<Value>> {
    let session = ctx
        .session
        .as_ref()
        .ok_or_else(|| ApiError::unauthorized("A user session is required"))?;
    Ok(Json(json!({
        "total": 1,
        "sessions": [session_json(session, true)],
    })))
}

pub async fn get_session(
    State(state): State<AppState>,
    ctx: ApiCtx,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    let session = if session_id == "current" {
        ctx.session
            .clone()
            .ok_or_else(|| ApiError::unauthorized("A user session is required"))?
    } else {
        find_session_by_id(&state, &ctx.project.id, &session_id)
            .await?
            .ok_or_else(|| ApiError::not_found("Session not found"))?
    };
    if session.user_id != user.id {
        return Err(ApiError::not_found("Session not found"));
    }
    Ok(Json(session_json(&session, true)))
}

pub async fn delete_session(
    State(state): State<AppState>,
    ctx: ApiCtx,
    jar: CookieJar,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> ApiResult<impl IntoResponse> {
    let user = ctx.require_user()?;
    let cookie_name = session_cookie_name(&ctx.project.id);
    if session_id == "current" {
        let token = ctx
            .session_token
            .as_ref()
            .ok_or_else(|| ApiError::unauthorized("A user session is required"))?;
        destroy_user_session(&state, &ctx.project.id, token, ctx.session.as_ref()).await?;
        let cookie = no_store_cookie(state.cfg(), cookie_name);
        return Ok((jar.add(cookie), StatusCode::NO_CONTENT));
    }
    let session = find_session_by_id(&state, &ctx.project.id, &session_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Session not found"))?;
    if session.user_id != user.id {
        return Err(ApiError::not_found("Session not found"));
    }
    crate::auth::delete_session_by_id(&state, &ctx.project.id, &session_id).await?;
    Ok((jar, StatusCode::NO_CONTENT))
}

pub async fn delete_sessions(
    State(state): State<AppState>,
    ctx: ApiCtx,
    jar: CookieJar,
) -> ApiResult<impl IntoResponse> {
    let user = ctx.require_user()?;
    destroy_all_user_sessions(&state, &ctx.project.id, &user.id).await?;
    let cookie = no_store_cookie(state.cfg(), session_cookie_name(&ctx.project.id));
    Ok((jar.add(cookie), StatusCode::NO_CONTENT))
}

/// A realtime-capable token for the current user session. Openwrite reuses the
/// session token itself; the realtime endpoint validates it the same way.
pub async fn create_jwt(State(state): State<AppState>, ctx: ApiCtx) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    let token = ctx.session_token.clone().ok_or_else(|| {
        ApiError::unauthorized("A user session is required to create a JWT")
    })?;
    rate_limit(&state, &format!("jwts:{}", user.id), 10).await?;
    Ok(Json(json!({ "jwt": token })))
}

/* ---------------- email verification / password recovery / MFA ---------------- */

#[derive(Deserialize)]
pub struct LinkBody {
    pub url: String,
}

#[derive(Deserialize)]
pub struct RecoveryRequest {
    pub email: String,
    pub url: String,
}

#[derive(Deserialize)]
pub struct SecretBody {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub secret: String,
}

#[derive(Deserialize)]
pub struct RecoveryConfirm {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub secret: String,
    pub password: String,
}

/// Only redirect to hosts this server trusts; otherwise fall back to PUBLIC_URL.
fn redirect_base(state: &AppState, url: &str) -> String {
    let cfg = state.cfg();
    let base = url.trim_end_matches('/');
    if cfg
        .cors_origins
        .iter()
        .any(|origin| base.starts_with(origin.trim_end_matches('/')))
        || base.starts_with(cfg.public_url.trim_end_matches('/'))
    {
        base.to_string()
    } else {
        cfg.public_url.trim_end_matches('/').to_string()
    }
}

pub async fn create_recovery(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Json(body): Json<RecoveryRequest>,
) -> ApiResult<Json<Value>> {
    let email = store::normalize_email(&body.email)?;
    rate_limit(
        &state,
        &format!("recovery:{}:{}", ctx.project.id, sha256_hex(&email)),
        5,
    )
    .await?;
    let link_base = redirect_base(&state, &body.url);
    if let Some(user) = store::find_user_by_email(&state.writer().await, &ctx.project.id, &email).await? {
        let secret = generate_token();
        store::create_auth_token(
            &state.writer().await,
            &ctx.project.id,
            &user.id,
            "recovery",
            &secret,
            3600,
        )
        .await?;
        let link = format!("{link_base}/recover?userId={}&secret={secret}", user.id);
        mailer::from_template(
            &state,
            "recovery",
            &user.email,
            &[
                ("{{link}}", link.as_str()),
                ("{{email}}", user.email.as_str()),
                ("{{appName}}", "Semester"),
            ],
            (
                "Reset your Semester password",
                "Open this link within an hour to choose a new password:\n\n{{link}}\n\nIf you did not ask for this, ignore the email.\n",
                "<p>Open this link within an hour to choose a new password:</p><p><a href=\"{{link}}\">{{link}}</a></p><p>If you did not ask for this, ignore this email.</p>",
            ),
        )
        .await?;
    }
    // same answer whether or not the account exists
    Ok(Json(json!({})))
}

pub async fn update_recovery(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Json(body): Json<RecoveryConfirm>,
) -> ApiResult<Json<Value>> {
    let pool = state.writer().await;
    let user = store::find_user(&pool, &ctx.project.id, &body.user_id)
        .await?
        .ok_or_else(|| ApiError::bad_request("Invalid or expired recovery link"))?;
    if !store::consume_auth_token(&pool, &ctx.project.id, &user.id, "recovery", &body.secret).await? {
        return Err(ApiError::bad_request("Invalid or expired recovery link"));
    }
    store::set_user_password(&pool, &ctx.project.id, &user.id, &body.password).await?;
    store::verify_email(&pool, &ctx.project.id, &user.id).await?;
    destroy_all_user_sessions(&state, &ctx.project.id, &user.id).await?;
    Ok(Json(json!({})))
}

pub async fn create_verification(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Json(body): Json<LinkBody>,
) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    let secret = generate_token();
    store::create_auth_token(
        &state.writer().await,
        &ctx.project.id,
        &user.id,
        "verification",
        &secret,
        3600,
    )
    .await?;
    let link = format!(
        "{}/verify?userId={}&secret={secret}",
        redirect_base(&state, &body.url),
        user.id
    );
    mailer::from_template(
        &state,
        "verification",
        &user.email,
        &[
            ("{{link}}", link.as_str()),
            ("{{email}}", user.email.as_str()),
            ("{{appName}}", "Semester"),
        ],
        (
            "Verify your Semester email",
            "Confirm your email address:\n\n{{link}}\n",
            "<p>Confirm your email address:</p><p><a href=\"{{link}}\">{{link}}</a></p>",
        ),
    )
    .await?;
    Ok(Json(json!({})))
}

pub async fn update_verification(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Json(body): Json<SecretBody>,
) -> ApiResult<Json<Value>> {
    let pool = state.writer().await;
    let user = store::find_user(&pool, &ctx.project.id, &body.user_id)
        .await?
        .ok_or_else(|| ApiError::bad_request("Invalid or expired verification link"))?;
    if !store::consume_auth_token(&pool, &ctx.project.id, &user.id, "verification", &body.secret).await? {
        return Err(ApiError::bad_request("Invalid or expired verification link"));
    }
    let user = store::verify_email(&pool, &ctx.project.id, &user.id).await?;
    Ok(Json(user_json(&user)))
}

pub async fn mfa_factors(ctx: ApiCtx) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    Ok(Json(json!({
        "totp": false,
        "email": user.email_verified,
        "phone": false,
    })))
}

#[derive(Deserialize)]
pub struct MfaBody {
    pub mfa: bool,
}

pub async fn mfa_update(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Json(body): Json<MfaBody>,
) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    if body.mfa && !user.email_verified {
        return Err(ApiError::bad_request("Verify your email address before enabling two-factor authentication"));
    }
    set_reserved_pref(&state, &ctx, "__mfa", json!(body.mfa)).await?;
    if !body.mfa {
        set_reserved_pref(&state, &ctx, "__mfaRecovery", json!([])).await?;
    }
    Ok(Json(json!({})))
}

async fn set_reserved_pref(
    state: &AppState,
    ctx: &ApiCtx,
    key: &str,
    value: Value,
) -> ApiResult<()> {
    store::set_reserved_pref(
        &state.writer().await,
        &ctx.project.id,
        ctx.require_user()?.id.as_str(),
        key,
        value,
    )
    .await
}

#[derive(Deserialize)]
pub struct FactorBody {
    pub factor: String,
}

pub async fn mfa_challenge_create(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Json(body): Json<FactorBody>,
) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    let factor = body.factor.to_lowercase();
    if factor != "email" && factor != "recoverycode" {
        return Err(ApiError::bad_request("Unsupported MFA factor"));
    }
    let (kind, secret) = if factor == "email" {
        let otp = rand::thread_rng().gen_range(100000..1000000).to_string();
        mailer::from_template(
            &state,
            "mfa",
            &user.email,
            &[
                ("{{code}}", otp.as_str()),
                ("{{email}}", user.email.as_str()),
                ("{{appName}}", "Semester"),
                ("{{agentClient}}", "this browser"),
                ("{{agentDevice}}", "your device"),
                ("{{agentOs}}", "an unrecognised device"),
            ],
            (
                "Your Semester sign-in code",
                "Your sign-in code is {{code}}. It expires in 10 minutes.\n",
                "<p>Your sign-in code is <strong>{{code}}</strong>. It expires in 10 minutes.</p>",
            ),
        )
        .await?;
        ("mfa_challenge", otp)
    } else {
        ("mfa_recovery", generate_token())
    };
    let id = store::create_auth_token(
        &state.writer().await,
        &ctx.project.id,
        &user.id,
        kind,
        &secret,
        600,
    )
    .await?;
    Ok(Json(json!({
        "$id": id,
        "userId": user.id,
        "expire": Utc::now().to_rfc3339(),
        "factor": factor,
    })))
}

#[derive(Deserialize)]
pub struct ChallengeBody {
    #[serde(rename = "challengeId")]
    pub challenge_id: String,
    pub otp: String,
}

pub async fn mfa_challenge_update(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Json(body): Json<ChallengeBody>,
) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    let id: Uuid = body.challenge_id.parse().map_err(|_| ApiError::bad_request("Invalid challenge"))?;
    let pool = state.writer().await;
    let token = store::get_auth_token(&pool, &ctx.project.id, &user.id, id)
        .await?
        .ok_or_else(|| ApiError::bad_request("Invalid challenge"))?;
    if token.used_at.is_some() || token.expires_at <= Utc::now() {
        return Err(ApiError::bad_request("Challenge expired — request a new code"));
    }
    if token.kind == "mfa_challenge" {
        if sha256_hex(&body.otp) != token.secret_hash {
            return Err(ApiError::bad_request("That code is not valid"));
        }
    } else if token.kind == "mfa_recovery" {
        let hash = sha256_hex(&body.otp);
        let codes = mfa_recovery_codes(&user);
        if !codes.contains(&hash) {
            return Err(ApiError::bad_request("That recovery code is not valid"));
        }
        let remaining: Vec<Value> = codes
            .into_iter()
            .filter(|code| code != &hash)
            .map(Value::String)
            .collect();
        set_reserved_pref(&state, &ctx, "__mfaRecovery", json!(remaining)).await?;
    } else {
        return Err(ApiError::bad_request("Invalid challenge"));
    }
    store::mark_auth_token_used(&pool, id).await?;
    if let Some(token_string) = &ctx.session_token {
        set_session_mfa_pending(&state, &ctx.project.id, token_string, false).await?;
    }
    Ok(Json(json!({})))
}

pub async fn mfa_recovery_codes_get(ctx: ApiCtx) -> ApiResult<Json<Value>> {
    let user = ctx.require_user()?;
    Ok(Json(json!({ "recoveryCodes": mfa_recovery_codes(&user) })))
}

pub async fn mfa_recovery_codes_create(
    State(state): State<AppState>,
    ctx: ApiCtx,
) -> ApiResult<Json<Value>> {
    let mut codes = Vec::new();
    let mut hashes = Vec::new();
    for _ in 0..10 {
        let code = generate_token()[..12].to_string();
        hashes.push(Value::String(sha256_hex(&code)));
        codes.push(code);
    }
    set_reserved_pref(&state, &ctx, "__mfaRecovery", json!(hashes)).await?;
    Ok(Json(json!({ "recoveryCodes": codes })))
}

#[allow(dead_code)]
fn _pool(_: PgPool) {}

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
};
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::{
    auth::{
        ApiCtx, AppState, build_cookie, create_user_session, destroy_all_user_sessions,
        destroy_other_user_sessions, destroy_user_session, find_session_by_id, no_store_cookie,
        rate_limit, session_cookie_name,
    },
    error::{ApiError, ApiResult},
    models::{UserRow, session_json, user_json},
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
    let prefs = match body.prefs {
        Value::Object(_) => body.prefs,
        _ => return Err(ApiError::bad_request("prefs must be an object")),
    };
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

#[allow(dead_code)]
fn _pool(_: PgPool) {}

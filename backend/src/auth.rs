use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::crypto::{NodeCipher, constant_time_eq_hex, sha256_hex};
use crate::error::{ApiError, ApiResult};
use crate::models::{ProjectRow, UserRow, UserSession};
use crate::nodes::NodeRegistry;

pub struct Inner {
    pub cfg: Config,
    pub nodes: Arc<RwLock<NodeRegistry>>,
    pub redis: redis::aio::ConnectionManager,
    pub http: reqwest::Client,
    pub cipher: NodeCipher,
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

impl AppState {
    pub fn new(
        cfg: Config,
        nodes: Arc<RwLock<NodeRegistry>>,
        redis: redis::aio::ConnectionManager,
        http: reqwest::Client,
        cipher: NodeCipher,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                cfg,
                nodes,
                redis,
                http,
                cipher,
            }),
        }
    }

    pub fn cfg(&self) -> &Config {
        &self.inner.cfg
    }

    pub fn nodes(&self) -> &Arc<RwLock<NodeRegistry>> {
        &self.inner.nodes
    }

    pub fn redis(&self) -> redis::aio::ConnectionManager {
        self.inner.redis.clone()
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.inner.http
    }

    pub fn cipher(&self) -> &NodeCipher {
        &self.inner.cipher
    }

    pub async fn writer(&self) -> PgPool {
        self.inner.nodes.read().await.writer.clone()
    }

    pub async fn reader(&self) -> PgPool {
        self.inner.nodes.read().await.reader()
    }
}

pub fn admin_cookie() -> &'static str {
    "ow_admin"
}

pub fn session_cookie_name(project_id: &str) -> String {
    format!("a_session_{project_id}")
}

pub fn build_cookie(
    cfg: &Config,
    name: String,
    value: String,
    max_age_days: i64,
) -> axum_extra::extract::cookie::Cookie<'static> {
    use axum_extra::extract::cookie::{Cookie, SameSite};

    let same_site = match cfg.cookie_samesite.as_str() {
        "strict" => SameSite::Strict,
        "none" => SameSite::None,
        _ => SameSite::Lax,
    };
    let mut builder = Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .same_site(same_site)
        .max_age(cookie::time::Duration::days(max_age_days))
        .secure(cfg.cookie_secure);
    if let Some(domain) = std::env::var("COOKIE_DOMAIN").ok().filter(|d| !d.is_empty()) {
        builder = builder.domain(domain);
    }
    builder.build()
}

pub fn no_store_cookie(cfg: &Config, name: String) -> axum_extra::extract::cookie::Cookie<'static> {
    use axum_extra::extract::cookie::{Cookie, SameSite};
    let same_site = match cfg.cookie_samesite.as_str() {
        "strict" => SameSite::Strict,
        "none" => SameSite::None,
        _ => SameSite::Lax,
    };
    Cookie::build((name, ""))
        .path("/")
        .http_only(true)
        .same_site(same_site)
        .secure(cfg.cookie_secure)
        .max_age(cookie::time::Duration::seconds(0))
        .build()
}

pub fn request_session_token(headers: &axum::http::HeaderMap, project_id: &str) -> Option<String> {
    if let Some(value) = headers
        .get("x-appwrite-session")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(value.to_string());
    }
    if let Some(value) = headers
        .get("x-appwrite-jwt")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(value.to_string());
    }
    if let Some(value) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(value.to_string());
    }
    axum_extra::extract::CookieJar::from_headers(headers)
        .get(&session_cookie_name(project_id))
        .map(|c| c.value().to_string())
}

pub struct AdminAuth {
    pub admin: serde_json::Value,
}

impl FromRequestParts<AppState> for AdminAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = axum_extra::extract::CookieJar::from_headers(&parts.headers)
            .get(admin_cookie())
            .map(|c| c.value().to_string())
            .ok_or_else(|| ApiError::unauthorized("Admin session required"))?;
        // The admin view lives in Redis so the console still authenticates while the
        // primary is fenced — exactly when a promotion has to happen.
        let admin = admin_session_lookup(state, &token)
            .await?
            .ok_or_else(|| ApiError::unauthorized("Admin session is invalid or expired"))?;
        Ok(Self { admin })
    }
}

pub struct ApiCtx {
    pub project: ProjectRow,
    pub is_api_key: bool,
    pub user: Option<UserRow>,
    pub session: Option<UserSession>,
    pub session_token: Option<String>,
}

impl ApiCtx {
    pub fn require_key(&self) -> ApiResult<()> {
        if self.is_api_key {
            Ok(())
        } else {
            Err(ApiError::unauthorized(
                "This endpoint requires a server API key (X-Appwrite-Key)",
            ))
        }
    }

    pub fn require_user(&self) -> ApiResult<&UserRow> {
        self.user.as_ref().ok_or_else(|| {
            ApiError::unauthorized("A user session is required for this endpoint")
        })
    }

    pub fn viewer_id(&self) -> Option<&str> {
        self.user.as_ref().map(|u| u.id.as_str())
    }
}

impl FromRequestParts<AppState> for ApiCtx {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let project_id = parts
            .headers
            .get("x-appwrite-project")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ApiError::bad_request("The X-Appwrite-Project header is required"))?;

        let writer = state.writer().await;
        let project = sqlx::query_as::<_, ProjectRow>(
            "select id, name, api_key_hash, api_key_prefix, created_at, updated_at from ow_projects where id = $1",
        )
        .bind(project_id)
        .fetch_optional(&writer)
        .await?
        .ok_or_else(|| ApiError::unauthorized("Project not found"))?;

        let mut is_api_key = false;
        if let Some(key) = parts
            .headers
            .get("x-appwrite-key")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            if !constant_time_eq_hex(&sha256_hex(key), &project.api_key_hash) {
                return Err(ApiError::unauthorized("Invalid API key"));
            }
            is_api_key = true;
        }

        let mut user = None;
        let mut session = None;
        let mut session_token = None;
        if !is_api_key {
            if let Some(token) = request_session_token(&parts.headers, &project.id) {
                if let Some(found) = session_lookup(state, &project.id, &token).await? {
                    if let Some(row) = sqlx::query_as::<_, UserRow>(
                        "select id, project_id, email, name, password_hash, prefs, status, imported, email_verified, registration, password_update, created_at, updated_at from ow_users where project_id = $1 and id = $2",
                    )
                    .bind(&project.id)
                    .bind(&found.user_id)
                    .fetch_optional(&writer)
                    .await?
                    {
                        if row.status {
                            user = Some(row);
                            session = Some(found);
                            session_token = Some(token);
                        }
                    }
                }
            }
        }

        Ok(Self {
            project,
            is_api_key,
            user,
            session,
            session_token,
        })
    }
}

fn admin_key(token: &str) -> String {
    format!("ow:admin:{}", sha256_hex(token))
}

pub async fn admin_session_lookup(
    state: &AppState,
    token: &str,
) -> ApiResult<Option<serde_json::Value>> {
    use redis::AsyncCommands;
    let mut conn = state.redis();
    let admin: Option<String> = conn.get(admin_key(token)).await?;
    Ok(admin.and_then(|v| serde_json::from_str(&v).ok()))
}

pub async fn create_admin_session(
    state: &AppState,
    admin: &serde_json::Value,
) -> ApiResult<String> {
    use redis::AsyncCommands;
    let token = crate::crypto::generate_token();
    let mut conn = state.redis();
    let _: () = conn
        .set_ex(admin_key(&token), admin.to_string(), 7 * 24 * 60 * 60)
        .await?;
    Ok(token)
}

pub async fn destroy_admin_session(state: &AppState, token: &str) -> ApiResult<()> {
    use redis::AsyncCommands;
    let mut conn = state.redis();
    let _: usize = conn.del(admin_key(token)).await?;
    Ok(())
}

fn session_key(project_id: &str, token: &str) -> String {
    format!("ow:session:{}:{}", project_id, sha256_hex(token))
}

fn user_sessions_key(project_id: &str, user_id: &str) -> String {
    format!("ow:usessions:{}:{}", project_id, user_id)
}

fn sid_key(project_id: &str, session_id: &str) -> String {
    format!("ow:sid:{}:{}", project_id, session_id)
}

pub async fn create_user_session(
    state: &AppState,
    project_id: &str,
    user_id: &str,
    email: &str,
) -> ApiResult<(String, UserSession)> {
    use redis::AsyncCommands;
    let token = crate::crypto::generate_token();
    let session_id = token[..20].to_string();
    let now = Utc::now();
    let session = UserSession {
        user_id: user_id.to_string(),
        session_id,
        email: email.to_string(),
        created_at: now,
        expire_at: now + Duration::days(365),
    };
    let key = session_key(project_id, &token);
    let digest = sha256_hex(&token);
    let payload = serde_json::to_string(&session)?;
    let mut conn = state.redis();
    let _: () = conn.set_ex(&key, payload, 365 * 24 * 60 * 60).await?;
    let index = user_sessions_key(project_id, user_id);
    let _: () = conn.sadd(&index, &digest).await?;
    let _: () = conn.expire(&index, 365 * 24 * 60 * 60).await?;
    let _: () = conn
        .set_ex(
            sid_key(project_id, &session.session_id),
            &digest,
            365 * 24 * 60 * 60,
        )
        .await?;
    Ok((token, session))
}

pub async fn session_lookup(
    state: &AppState,
    project_id: &str,
    token: &str,
) -> ApiResult<Option<UserSession>> {
    use redis::AsyncCommands;
    let mut conn = state.redis();
    let value: Option<String> = conn.get(session_key(project_id, token)).await?;
    Ok(value.and_then(|v| serde_json::from_str(&v).ok()))
}

pub async fn destroy_user_session(
    state: &AppState,
    project_id: &str,
    token: &str,
    session: Option<&UserSession>,
) -> ApiResult<()> {
    use redis::AsyncCommands;
    let mut conn = state.redis();
    let _: usize = conn.del(session_key(project_id, token)).await?;
    if let Some(session) = session {
        let digest = sha256_hex(token);
        let _: usize = conn
            .srem(user_sessions_key(project_id, &session.user_id), &digest)
            .await?;
        let _: usize = conn.del(sid_key(project_id, &session.session_id)).await?;
    }
    Ok(())
}

pub async fn destroy_all_user_sessions(
    state: &AppState,
    project_id: &str,
    user_id: &str,
) -> ApiResult<()> {
    use redis::AsyncCommands;
    let index = user_sessions_key(project_id, user_id);
    let mut conn = state.redis();
    let digests: Vec<String> = conn.smembers(&index).await?;
    for digest in digests {
        let key = format!("ow:session:{}:{}", project_id, digest);
        if let Ok(Some(payload)) = conn.get::<_, Option<String>>(&key).await {
            if let Ok(session) = serde_json::from_str::<UserSession>(&payload) {
                let _: usize = conn.del(sid_key(project_id, &session.session_id)).await?;
            }
        }
        let _: usize = conn.del(&key).await?;
    }
    let _: usize = conn.del(&index).await?;
    Ok(())
}

pub async fn rate_limit(state: &AppState, key: &str, max: i64) -> ApiResult<()> {
    use redis::AsyncCommands;
    let redis_key = format!("ow:ratelimit:{key}");
    let mut conn = state.redis();
    let count: i64 = conn.incr(&redis_key, 1i64).await?;
    if count == 1 {
        let _: () = conn.expire(&redis_key, 900).await?;
    }
    if count > max {
        return Err(ApiError::too_many(
            "Too many attempts. Try again in a few minutes.",
        ));
    }
    Ok(())
}

pub async fn clear_rate_limit(state: &AppState, key: &str) -> ApiResult<()> {
    use redis::AsyncCommands;
    let mut conn = state.redis();
    let _: usize = conn.del(format!("ow:ratelimit:{key}")).await?;
    Ok(())
}


pub async fn destroy_other_user_sessions(
    state: &AppState,
    project_id: &str,
    user_id: &str,
    current_token: &str,
) -> ApiResult<()> {
    use redis::AsyncCommands;
    let index = user_sessions_key(project_id, user_id);
    let current = sha256_hex(current_token);
    let mut conn = state.redis();
    let digests: Vec<String> = conn.smembers(&index).await?;
    for digest in digests {
        if digest == current {
            continue;
        }
        let key = format!("ow:session:{}:{}", project_id, digest);
        if let Ok(Some(payload)) = conn.get::<_, Option<String>>(&key).await {
            if let Ok(session) = serde_json::from_str::<UserSession>(&payload) {
                let _: usize = conn.del(sid_key(project_id, &session.session_id)).await?;
            }
        }
        let _: usize = conn.del(&key).await?;
        let _: usize = conn.srem(&index, &digest).await?;
    }
    Ok(())
}

pub async fn find_session_by_id(
    state: &AppState,
    project_id: &str,
    session_id: &str,
) -> ApiResult<Option<UserSession>> {
    use redis::AsyncCommands;
    let mut conn = state.redis();
    let digest: Option<String> = conn.get(sid_key(project_id, session_id)).await?;
    let Some(digest) = digest else {
        return Ok(None);
    };
    let payload: Option<String> = conn
        .get(format!("ow:session:{}:{}", project_id, digest))
        .await?;
    Ok(payload.and_then(|v| serde_json::from_str(&v).ok()))
}

pub async fn delete_session_by_id(
    state: &AppState,
    project_id: &str,
    session_id: &str,
) -> ApiResult<bool> {
    use redis::AsyncCommands;
    let session = find_session_by_id(state, project_id, session_id).await?;
    let Some(session) = session else {
        return Ok(false);
    };
    let mut conn = state.redis();
    let digest: Option<String> = conn.get(sid_key(project_id, session_id)).await?;
    if let Some(digest) = digest {
        let _: usize = conn
            .del(format!("ow:session:{}:{}", project_id, digest))
            .await?;
        let _: usize = conn
            .srem(
                user_sessions_key(project_id, &session.user_id),
                &digest,
            )
            .await?;
    }
    let _: usize = conn.del(sid_key(project_id, session_id)).await?;
    Ok(true)
}

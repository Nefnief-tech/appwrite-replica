use axum::{
    Json,
    extract::{Path, RawQuery, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    auth::{ApiCtx, AppState},
    error::{ApiError, ApiResult},
    models::user_json,
    store,
};

#[derive(Deserialize)]
pub struct CreateUserBody {
    #[serde(rename = "userId")]
    pub user_id: Option<String>,
    pub email: String,
    pub password: Option<String>,
    pub name: Option<String>,
}

pub async fn list_users(
    State(state): State<AppState>,
    ctx: ApiCtx,
    RawQuery(query): RawQuery,
) -> ApiResult<Json<Value>> {
    ctx.require_key()?;
    let mut limit = 25i64;
    let mut offset = 0i64;
    let mut search = String::new();
    if let Some(query) = query {
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            match key.as_ref() {
                "limit" => limit = value.parse().unwrap_or(25),
                "offset" => offset = value.parse().unwrap_or(0),
                "search" => search = value.to_string(),
                "queries" | "queries[]" => {
                    let text = value.to_string();
                    if let Some(rest) = text.strip_prefix("limit(").and_then(|v| v.strip_suffix(')')) {
                        limit = rest.trim_matches('"').parse().unwrap_or(limit);
                    } else if let Some(rest) =
                        text.strip_prefix("offset(").and_then(|v| v.strip_suffix(')'))
                    {
                        offset = rest.trim_matches('"').parse().unwrap_or(offset);
                    } else if let Some(rest) =
                        text.strip_prefix("search(").and_then(|v| v.strip_suffix(')'))
                    {
                        search = rest
                            .split(',')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .trim_matches('"')
                            .to_string();
                    }
                }
                _ => {}
            }
        }
    }
    let (total, users) = store::list_users(
        &state.writer().await,
        &ctx.project.id,
        limit,
        offset,
        &search,
    )
    .await?;
    Ok(Json(json!({
        "total": total,
        "users": users.iter().map(user_json).collect::<Vec<_>>(),
    })))
}

pub async fn create_user(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Json(body): Json<CreateUserBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ctx.require_key()?;
    let user = store::create_user(
        &state.writer().await,
        &ctx.project.id,
        body.user_id,
        &body.email,
        body.name.as_deref().unwrap_or(""),
        body.password,
        false,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(user_json(&user))))
}

pub async fn get_user(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path(user_id): Path<String>,
) -> ApiResult<Json<Value>> {
    ctx.require_key()?;
    let user = store::find_user(&state.writer().await, &ctx.project.id, &user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("User not found"))?;
    Ok(Json(user_json(&user)))
}

pub async fn delete_user(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path(user_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    ctx.require_key()?;
    store::delete_user(&state.writer().await, &ctx.project.id, &user_id).await?;
    crate::auth::destroy_all_user_sessions(&state, &ctx.project.id, &user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

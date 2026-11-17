use std::time::Instant;

use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};

use crate::auth::AppState;

pub async fn root() -> Json<Value> {
    Json(json!({
        "name": "Openwrite",
        "description": "A modular Appwrite-compatible backend on Postgres and Redis",
        "version": env!("CARGO_PKG_VERSION"),
        "api": "/v1",
    }))
}

pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    let started = Instant::now();
    let db_ok = sqlx::query("select 1")
        .execute(&state.writer().await)
        .await
        .is_ok();
    let mut redis = state.redis();
    let redis_ok = redis::cmd("PING")
        .query_async::<String>(&mut redis)
        .await
        .is_ok();
    let status = if db_ok && redis_ok { "pass" } else { "fail" };
    let code = if db_ok && redis_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        code,
        Json(json!({
            "name": "openwrite",
            "version": env!("CARGO_PKG_VERSION"),
            "status": status,
            "ping": started.elapsed().as_millis() as u64,
            "services": {
                "database": db_ok,
                "redis": redis_ok,
            }
        })),
    )
}

pub async fn version() -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn ping() -> Json<Value> {
    Json(json!({
        "code": 200,
    }))
}

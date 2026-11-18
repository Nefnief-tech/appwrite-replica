mod auth;
mod config;
mod crypto;
mod db;
mod docker;
mod documents;
mod error;
mod mailer;
mod models;
mod nodes;
mod realtime;
mod routes;
mod schema;
mod store;
mod usage;

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::Response,
};
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;

use crate::{
    auth::AppState,
    config::Config,
    crypto::NodeCipher,
};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "openwrite_api=info,tower_http=info".into()),
        )
        .init();

    let cfg = Config::from_env()?;
    let cipher = NodeCipher::new(&cfg.node_secret);

    let saved_leader = nodes::load_saved_leader(&cfg, &cipher)
        .context("could not read the saved leader file")?;
    let mut active_dsn = saved_leader
        .clone()
        .unwrap_or_else(|| cfg.database_url.clone());

    let control = match db::connect_control_plane(&active_dsn).await {
        Ok(pool) => pool,
        Err(err) if saved_leader.is_some() => {
            tracing::warn!("saved leader is unreachable ({err}); falling back to DATABASE_URL");
            active_dsn = cfg.database_url.clone();
            db::connect_control_plane(&active_dsn).await?
        }
        Err(err) => return Err(err),
    };

    db::ensure_admin(&control, &cfg).await?;
    nodes::seed_nodes(&control, &cipher, &active_dsn, &cfg.followers).await?;
    let registry = nodes::load_registry(&control, &cipher, cfg.read_routing).await?;
    tracing::info!("active leader: {}", registry.writer_id);

    if saved_leader.is_none() {
        if let Err(err) = nodes::save_leader(&cfg, &cipher, &active_dsn) {
            tracing::warn!("could not save the leader file: {err}");
        }
    }

    let registry = Arc::new(RwLock::new(registry));
    nodes::check_all(&registry).await;
    tokio::spawn(nodes::health_loop(registry.clone()));
    tokio::spawn(usage::retention_loop(control.clone(), cfg.usage_retention_days));
    tokio::spawn(realtime::listener_task(control.clone()));

    let redis_client =
        redis::Client::open(cfg.redis_url.clone()).context("invalid REDIS_URL")?;
    let redis = redis_client
        .get_connection_manager()
        .await
        .context("could not connect to Redis")?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .context("could not create the HTTP client")?;

    let state = AppState::new(cfg.clone(), registry, redis, http, cipher);
    let app: Router = Router::new()
        .merge(
            routes::api_router()
                .layer(axum::middleware::from_fn_with_state(state.clone(), usage::record)),
        )
        .nest("/admin/api", routes::admin_router())
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn_with_state(state.clone(), cors))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr)
        .await
        .with_context(|| format!("could not bind {}", cfg.bind_addr))?;
    tracing::info!("Openwrite listening on {}", cfg.bind_addr);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .context("server error")?;
    Ok(())
}

fn origin_allowed(cfg: &Config, origin: &HeaderValue) -> bool {
    let Ok(value) = origin.to_str() else {
        return false;
    };
    if cfg.cors_origins.is_empty() {
        // Development default: reflect any origin so credentialed SDK requests
        // work out of the box. Set CORS_ORIGINS to restrict this.
        return true;
    }
    cfg.cors_origins
        .iter()
        .any(|allowed| allowed.trim_end_matches('/').eq_ignore_ascii_case(value))
}

/// CORS as a plain middleware: `Allow-Origin: *` cannot carry credentials, so
/// the caller's origin is reflected instead. Preflights are answered here and
/// never reach the routes.
async fn cors(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let preflight = req.method() == Method::OPTIONS
        && req.headers().contains_key("access-control-request-method");
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .filter(|origin| origin_allowed(&state.cfg(), origin))
        .cloned();
    let requested_headers = req
        .headers()
        .get("access-control-request-headers")
        .cloned();

    let mut response = if preflight {
        Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(axum::body::Body::empty())
            .expect("static preflight response")
    } else {
        next.run(req).await
    };

    if let Some(origin) = origin {
        let headers = response.headers_mut();
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
        headers.append(header::VARY, HeaderValue::from_static("Origin"));
        if preflight {
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE, OPTIONS"),
            );
            if let Some(requested) = requested_headers {
                headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, requested);
            }
        }
    }
    response
}

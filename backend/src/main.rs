mod auth;
mod config;
mod crypto;
mod db;
mod documents;
mod error;
mod models;
mod nodes;
mod routes;
mod schema;
mod store;

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method},
};
use tokio::sync::RwLock;
use tower_http::{
    cors::{AllowOrigin, Any, CorsLayer},
    trace::TraceLayer,
};

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
        .merge(routes::api_router())
        .nest("/admin/api", routes::admin_router())
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(cors(&cfg))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr)
        .await
        .with_context(|| format!("could not bind {}", cfg.bind_addr))?;
    tracing::info!("Openwrite listening on {}", cfg.bind_addr);
    axum::serve(listener, app)
        .await
        .context("server error")?;
    Ok(())
}

fn cors(cfg: &Config) -> CorsLayer {
    let layer = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(Any);
    if cfg.cors_origins.is_empty() {
        return layer.allow_origin(Any);
    }
    let origins: Vec<HeaderValue> = cfg
        .cors_origins
        .iter()
        .filter_map(|origin| origin.parse().ok())
        .collect();
    layer
        .allow_origin(AllowOrigin::list(origins))
        .allow_credentials(true)
}

use axum::{Json, extract::{Path, State}, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};
use uuid::Uuid;

use crate::{
    auth::{AdminAuth, AppState},
    crypto::dsn_hint,
    docker::{Docker, NAME_PREFIX},
    error::{ApiError, ApiResult},
    nodes::{NodeRole, lazy_pool},
    routes::admin,
};

pub async fn status(State(state): State<AppState>, _admin: AdminAuth) -> Json<Value> {
    let docker = Docker::new(state.cfg());
    match docker.version().await {
        Ok(version) => Json(json!({
            "available": true,
            "version": version,
            "mode": format!("{:?}", state.cfg().docker_replica_mode).to_lowercase(),
        })),
        Err(err) => Json(json!({ "available": false, "error": err.to_string() })),
    }
}

#[derive(Deserialize)]
pub struct SpawnBody {
    pub name: Option<String>,
}

pub async fn spawn_replica(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(body): Json<SpawnBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if !Docker::new(state.cfg()).version().await.is_ok() {
        return Err(ApiError::bad_request("Docker is not available on this host"));
    }
    let suffix = match &body.name {
        Some(raw) => {
            let cleaned: String = raw
                .trim()
                .to_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
                .collect();
            let cleaned = cleaned.trim_matches('-').chars().take(24).collect::<String>();
            if cleaned.is_empty() {
                return Err(ApiError::bad_request("Replica name must contain letters or digits"));
            }
            cleaned
        }
        None => Uuid::new_v4().to_string()[..8].to_string(),
    };
    let name = format!("{NAME_PREFIX}{suffix}");

    let leader_dsn = {
        let registry = state.nodes().read().await;
        let Some(leader) = registry.nodes.iter().find(|node| {
            node.role.try_read().map(|role| *role == NodeRole::Leader).unwrap_or(false)
        }) else {
            return Err(ApiError::bad_request("No leader node is registered"));
        };
        // NodeRuntime.dsn is already decrypted by load_registry.
        leader.dsn.clone()
    };

    let docker = Docker::new(state.cfg());
    let (container_id, dsn) = docker
        .spawn_replica(&leader_dsn, &name)
        .await
        .map_err(|err| ApiError::bad_request(format!("could not start replica: {err}")))?;
    let hint = dsn_hint(&dsn);

    tracing::info!("spawned replica {name} at {} (container {container_id})", dsn_hint(&dsn));
    let pool = lazy_pool(&dsn, 2).map_err(|err| ApiError::bad_request(err.to_string()))?;
    let mut ready = false;
    for _ in 0..45 {
        if let Ok(in_recovery) =
            sqlx::query_scalar::<_, bool>("select pg_is_in_recovery()").fetch_one(&pool).await
        {
            if in_recovery {
                ready = true;
                break;
            }
        }
        sleep(Duration::from_secs(2)).await;
    }
    if !ready {
        let _ = docker.drop_container(&container_id).await;
        return Err(ApiError::bad_request(
            "Replica did not become ready in time; its container was removed",
        ));
    }

    let node_id = match admin::insert_node(&state, &name, &dsn, "follower").await {
        Ok(id) => id,
        Err(err) => {
            let _ = docker.drop_container(&container_id).await;
            return Err(err);
        }
    };
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": node_id,
            "name": name,
            "dsnHint": hint,
            "containerId": container_id,
        })),
    ))
}

pub async fn remove_replica(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(node_id): Path<String>,
) -> ApiResult<StatusCode> {
    let id = Uuid::parse_str(&node_id)
        .map_err(|_| ApiError::bad_request("Invalid node ID"))?;
    let name = {
        let registry = state.nodes().read().await;
        let Some(node) = registry.nodes.iter().find(|node| node.id == id) else {
            return Err(ApiError::not_found("Node not found"));
        };
        node.name.clone()
    };
    if !name.starts_with(NAME_PREFIX) {
        return Err(ApiError::bad_request("This node was not created by Docker"));
    }
    let docker = Docker::new(state.cfg());
    if let Some(container_id) = docker.container_for_node(&name).await? {
        docker.drop_container(&container_id).await.map_err(|err| {
            ApiError::bad_request(format!("could not remove replica container: {err}"))
        })?;
    }
    admin::delete_node(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

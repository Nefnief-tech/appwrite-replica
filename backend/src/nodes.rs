use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{
    Connection as _, PgPool,
    postgres::PgPoolOptions,
};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    config::ReadRouting,
    crypto::{NodeCipher, dsn_hint},
    error::{ApiError, ApiResult},
    models::ts,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    Leader,
    Follower,
    Retired,
}

impl NodeRole {
    pub fn from_str(value: &str) -> Self {
        match value {
            "leader" => Self::Leader,
            "follower" => Self::Follower,
            _ => Self::Retired,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeStatus {
    pub ok: bool,
    pub latency_ms: Option<u64>,
    pub in_recovery: Option<bool>,
    pub lag_seconds: Option<f64>,
    pub lag_bytes: Option<u64>,
    pub read_only: Option<bool>,
    pub error: Option<String>,
    pub checked_at: Option<String>,
}

impl Default for NodeStatus {
    fn default() -> Self {
        Self {
            ok: false,
            latency_ms: None,
            in_recovery: None,
            lag_seconds: None,
            lag_bytes: None,
            read_only: None,
            error: Some("Not checked yet".into()),
            checked_at: None,
        }
    }
}

pub struct NodeRuntime {
    pub id: Uuid,
    pub name: String,
    pub role: RwLock<NodeRole>,
    pub dsn: String,
    pub dsn_hint: String,
    pub pool: PgPool,
    pub status: RwLock<NodeStatus>,
    pub created_at: DateTime<Utc>,
}

pub struct NodeRegistry {
    pub writer_id: Uuid,
    pub writer: PgPool,
    pub nodes: Vec<Arc<NodeRuntime>>,
    pub read_routing: ReadRouting,
    rr: std::sync::atomic::AtomicUsize,
}

impl NodeRegistry {
    pub fn find(&self, id: Uuid) -> Option<Arc<NodeRuntime>> {
        self.nodes.iter().find(|n| n.id == id).cloned()
    }

    pub fn reader(&self) -> PgPool {
        if self.read_routing != ReadRouting::Replica {
            return self.writer.clone();
        }
        // Rotation over followers that recently answered a health check.
        let attempts = self.nodes.len().max(1);
        for _ in 0..attempts {
            let idx = self
                .rr
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                % attempts;
            let node = &self.nodes[idx];
            if node.id == self.writer_id {
                continue;
            }
            if let Ok(role) = node.role.try_read() {
                if *role != NodeRole::Follower {
                    continue;
                }
            }
            if let Ok(status) = node.status.try_read() {
                if status.ok && status.in_recovery == Some(true) {
                    return node.pool.clone();
                }
            }
        }
        self.writer.clone()
    }

    pub async fn view(&self) -> Vec<Value> {
        let mut views = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let role = *node.role.read().await;
            let status = node.status.read().await.clone();
            views.push(json!({
                "id": node.id,
                "name": node.name,
                "role": role,
                "dsnHint": node.dsn_hint,
                "isWriter": node.id == self.writer_id,
                "createdAt": ts(node.created_at),
                "status": status,
            }));
        }
        views.sort_by_key(|v| {
            let role = v.get("role").and_then(Value::as_str).unwrap_or("");
            match role {
                "leader" => 0,
                "follower" => 1,
                _ => 2,
            }
        });
        views
    }
}

pub fn lazy_pool(dsn: &str, max_connections: u32) -> Result<PgPool> {
    Ok(PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(6))
        .connect_lazy(dsn)
        .context("invalid PostgreSQL DSN")?)
}

pub async fn seed_nodes(
    pool: &PgPool,
    cipher: &NodeCipher,
    database_url: &str,
    followers: &[String],
) -> Result<()> {
    let count: i64 = sqlx::query_scalar("select count(*) from ow_nodes")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Ok(());
    }

    sqlx::query("insert into ow_nodes (id, name, dsn, role) values ($1, 'primary', $2, 'leader')")
        .bind(Uuid::new_v4())
        .bind(cipher.encrypt(database_url)?)
        .execute(pool)
        .await?;

    for (index, dsn) in followers.iter().enumerate() {
        if crate::crypto::validate_dsn(dsn).is_err() {
            tracing::warn!("ignoring invalid follower DSN at index {index}");
            continue;
        }
        sqlx::query("insert into ow_nodes (id, name, dsn, role) values ($1, $2, $3, 'follower')")
            .bind(Uuid::new_v4())
            .bind(format!("replica-{}", index + 1))
            .bind(cipher.encrypt(dsn)?)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn load_registry(
    pool: &PgPool,
    cipher: &NodeCipher,
    read_routing: ReadRouting,
) -> Result<NodeRegistry> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        name: String,
        dsn: String,
        role: String,
        created_at: DateTime<Utc>,
    }

    let rows: Vec<Row> = sqlx::query_as(
        "select id, name, dsn, role, created_at from ow_nodes order by created_at asc",
    )
    .fetch_all(pool)
    .await?;

    let mut nodes = Vec::new();
    for row in rows {
        let dsn = cipher.decrypt(&row.dsn)?;
        let node = Arc::new(NodeRuntime {
            id: row.id,
            name: row.name,
            role: RwLock::new(NodeRole::from_str(&row.role)),
            dsn_hint: dsn_hint(&dsn),
            pool: lazy_pool(&dsn, 8)?,
            dsn,
            status: RwLock::new(NodeStatus::default()),
            created_at: row.created_at,
        });
        nodes.push(node);
    }

    let leader = nodes
        .iter()
        .find(|n| matches!(*n.role.try_read().expect("fresh lock"), NodeRole::Leader))
        .cloned()
        .context("no leader node is registered; add one with DATABASE_URL")?;

    // Verify the leader is reachable before serving traffic.
    sqlx::query("select 1")
        .execute(&leader.pool)
        .await
        .context("leader database is unreachable")?;

    Ok(NodeRegistry {
        writer_id: leader.id,
        writer: leader.pool.clone(),
        nodes,
        read_routing,
        rr: std::sync::atomic::AtomicUsize::new(0),
    })
}

pub fn parse_lsn(value: &str) -> Option<u64> {
    let (hi, lo) = value.split_once('/')?;
    let hi = u64::from_str_radix(hi, 16).ok()?;
    let lo = u64::from_str_radix(lo, 16).ok()?;
    Some((hi << 32) | lo)
}

async fn leader_lsn(writer: &PgPool) -> Option<u64> {
    let value: Option<String> = sqlx::query_scalar("select pg_current_wal_lsn()::text")
        .fetch_one(writer)
        .await
        .ok();
    value.as_deref().and_then(parse_lsn)
}

pub async fn check_node(node: &NodeRuntime, leader_lsn: Option<u64>) -> NodeStatus {
    let started = std::time::Instant::now();
    let result: Result<NodeStatus, sqlx::Error> = async {
        let role = *node.role.read().await;
        match role {
            NodeRole::Leader => {
                let row: (bool, Option<String>, String) = sqlx::query_as(
                    "select pg_is_in_recovery(), pg_current_wal_lsn()::text, current_setting('transaction_read_only')",
                )
                .fetch_one(&node.pool)
                .await?;
                Ok(NodeStatus {
                    ok: !row.0 && row.2 == "off",
                    latency_ms: Some(started.elapsed().as_millis() as u64),
                    in_recovery: Some(row.0),
                    lag_seconds: None,
                    lag_bytes: Some(0),
                    read_only: Some(row.2 == "on"),
                    error: if row.0 {
                        Some("Node is in recovery but registered as leader".into())
                    } else if row.2 == "on" {
                        Some("Node is read-only".into())
                    } else {
                        None
                    },
                    checked_at: Some(ts(Utc::now())),
                })
            }
            NodeRole::Follower => {
                let row: (bool, Option<String>, String, Option<f64>) = sqlx::query_as(
                    "select pg_is_in_recovery(), pg_last_wal_replay_lsn()::text, current_setting('transaction_read_only'), extract(epoch from (now() - pg_last_xact_replay_timestamp()))::float8",
                )
                .fetch_one(&node.pool)
                .await?;
                let replay = row.1.as_deref().and_then(parse_lsn);
                let lag_bytes = match (leader_lsn, replay) {
                    (Some(leader), Some(replay)) => Some(leader.saturating_sub(replay)),
                    _ => None,
                };
                let mut error = None;
                if !row.0 {
                    error = Some("Node is writable but registered as a follower".into());
                }
                Ok(NodeStatus {
                    ok: row.0,
                    latency_ms: Some(started.elapsed().as_millis() as u64),
                    in_recovery: Some(row.0),
                    lag_seconds: row.3.filter(|s| *s >= 0.0),
                    lag_bytes,
                    read_only: Some(row.2 == "on"),
                    error,
                    checked_at: Some(ts(Utc::now())),
                })
            }
            NodeRole::Retired => {
                let row: (bool, String) = sqlx::query_as(
                    "select pg_is_in_recovery(), current_setting('transaction_read_only')",
                )
                .fetch_one(&node.pool)
                .await?;
                Ok(NodeStatus {
                    ok: false,
                    latency_ms: Some(started.elapsed().as_millis() as u64),
                    in_recovery: Some(row.0),
                    lag_seconds: None,
                    lag_bytes: None,
                    read_only: Some(row.1 == "on"),
                    error: Some("Retired after failover. Rebuild as a replica before use.".into()),
                    checked_at: Some(ts(Utc::now())),
                })
            }
        }
    }
    .await;

    match result {
        Ok(mut status) => {
            status.checked_at = Some(ts(Utc::now()));
            status
        }
        Err(err) => NodeStatus {
            ok: false,
            latency_ms: Some(started.elapsed().as_millis() as u64),
            error: Some(err.to_string()),
            checked_at: Some(ts(Utc::now())),
            ..NodeStatus::default()
        },
    }
}

pub async fn check_all(registry: &RwLock<NodeRegistry>) {
    let (nodes, writer) = {
        let reg = registry.read().await;
        (reg.nodes.clone(), reg.writer.clone())
    };
    let lsn = leader_lsn(&writer).await;
    let statuses = join_all(nodes.iter().map(|node| check_node(node, lsn))).await;
    for (node, status) in nodes.iter().zip(statuses) {
        *node.status.write().await = status;
    }
}

pub async fn health_loop(registry: Arc<RwLock<NodeRegistry>>) {
    loop {
        check_all(&registry).await;
        tokio::time::sleep(Duration::from_secs(20)).await;
    }
}

pub async fn check_single(registry: &RwLock<NodeRegistry>, id: Uuid) -> ApiResult<Value> {
    let (node, lsn) = {
        let reg = registry.read().await;
        let node = reg
            .find(id)
            .ok_or_else(|| ApiError::not_found("Node not found"))?;
        let lsn = if reg.writer_id == id {
            None
        } else {
            leader_lsn(&reg.writer).await
        };
        (node, lsn)
    };
    let status = check_node(&node, lsn).await;
    *node.status.write().await = status.clone();
    let role = *node.role.read().await;
    Ok(json!({
        "id": node.id,
        "name": node.name,
        "role": role,
        "dsnHint": node.dsn_hint,
        "isWriter": id == registry.read().await.writer_id,
        "createdAt": ts(node.created_at),
        "status": status,
    }))
}

#[cfg(test)]
mod tests {
    use super::parse_lsn;

    #[test]
    fn parses_pg_lsn() {
        assert_eq!(parse_lsn("0/16B6C50"), Some(0x16B6C50));
        assert_eq!(parse_lsn("1/0"), Some(1 << 32));
        assert_eq!(parse_lsn("nope"), None);
    }
}

pub fn saved_leader_path(cfg: &crate::config::Config) -> std::path::PathBuf {
    cfg.data_dir.join("leader")
}

pub fn load_saved_leader(cfg: &crate::config::Config, cipher: &NodeCipher) -> Result<Option<String>> {
    let path = saved_leader_path(cfg);
    if !path.exists() {
        return Ok(None);
    }
    let encoded = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;
    let dsn = cipher
        .decrypt(encoded.trim())
        .with_context(|| format!("could not decrypt {}", path.display()))?;
    Ok(Some(dsn))
}

pub fn save_leader(cfg: &crate::config::Config, cipher: &NodeCipher, dsn: &str) -> Result<()> {
    std::fs::create_dir_all(&cfg.data_dir)
        .with_context(|| format!("could not create {}", cfg.data_dir.display()))?;
    let path = saved_leader_path(cfg);
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, cipher.encrypt(dsn)?)
        .with_context(|| format!("could not write {}", temporary.display()))?;
    std::fs::rename(&temporary, &path)
        .with_context(|| format!("could not replace {}", path.display()))?;
    Ok(())
}

pub async fn verify_writable(pool: &PgPool) -> ApiResult<()> {
    let mut connection = pool
        .acquire()
        .await
        .map_err(|err| ApiError::bad_request(format!("Node is unreachable: {err}")))?;
    let recovery: bool = sqlx::query_scalar("select pg_is_in_recovery()")
        .fetch_one(&mut *connection)
        .await
        .map_err(|err| ApiError::bad_request(format!("Could not read node state: {err}")))?;
    if recovery {
        return Err(ApiError::bad_request(
            "This node is still in recovery. Promote it at the PostgreSQL level first (SELECT pg_promote();), then try again.",
        ));
    }
    let read_only: String = sqlx::query_scalar("select current_setting('transaction_read_only')")
        .fetch_one(&mut *connection)
        .await
        .map_err(|err| ApiError::bad_request(format!("Could not read node state: {err}")))?;
    if read_only == "on" {
        return Err(ApiError::bad_request(
            "This node is read-only. Enable writes at the PostgreSQL level first.",
        ));
    }
    let mut tx = connection
        .begin()
        .await
        .map_err(|err| ApiError::bad_request(format!("Could not start a write probe: {err}")))?;
    sqlx::query("create temp table ow_write_probe (x integer) on commit drop")
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            ApiError::bad_request(format!("Node is not accepting writes: {err}"))
        })?;
    tx.rollback().await.ok();
    Ok(())
}

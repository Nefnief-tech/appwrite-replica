use std::{env, path::PathBuf};

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub followers: Vec<String>,
    pub redis_url: String,
    pub bind_addr: String,
    pub public_url: String,
    pub data_dir: PathBuf,
    pub node_secret: String,
    pub admin_email: String,
    pub admin_password: String,
    pub cors_origins: Vec<String>,
    pub cookie_secure: bool,
    pub cookie_samesite: String,
    pub read_routing: ReadRouting,
    pub import_max_documents: u64,
    pub usage_retention_days: u32,
    pub docker_socket: PathBuf,
    pub docker_replica_mode: DockerReplicaMode,
    pub docker_network: String,
    pub docker_replica_host: String,
    pub docker_leader_host: Option<String>,
    pub docker_leader_port: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockerReplicaMode {
    /// Publish a random localhost port and connect to it from the API host.
    Host,
    /// Attach to the API's Docker network and connect by container name.
    Network,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadRouting {
    Leader,
    Replica,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
        let session_secret =
            env::var("SESSION_SECRET").context("SESSION_SECRET is required")?;
        if session_secret.len() < 24 {
            anyhow::bail!("SESSION_SECRET must be at least 24 characters");
        }

        let followers = env::var("DATABASE_FOLLOWERS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();

        let cors_origins = env::var("CORS_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();

        let cookie_samesite = env::var("COOKIE_SAMESITE")
            .unwrap_or_else(|_| "lax".into())
            .to_lowercase();
        if !matches!(cookie_samesite.as_str(), "lax" | "strict" | "none") {
            anyhow::bail!("COOKIE_SAMESITE must be lax, strict or none");
        }

        let read_routing = match env::var("READ_ROUTING").unwrap_or_else(|_| "leader".into()).as_str() {
            "replica" => ReadRouting::Replica,
            "leader" => ReadRouting::Leader,
            other => anyhow::bail!("READ_ROUTING must be leader or replica, got {other}"),
        };

        Ok(Self {
            database_url,
            followers,
            redis_url: env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into()),
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            public_url: env::var("PUBLIC_URL").unwrap_or_else(|_| "http://localhost:8080".into()),
            data_dir: PathBuf::from(env::var("DATA_DIR").unwrap_or_else(|_| "./data".into())),
            node_secret: env::var("NODE_SECRET").unwrap_or_else(|_| session_secret.clone()),
            admin_email: env::var("ADMIN_EMAIL").context("ADMIN_EMAIL is required")?,
            admin_password: env::var("ADMIN_PASSWORD").context("ADMIN_PASSWORD is required")?,
            cors_origins,
            cookie_secure: env::var("COOKIE_SECURE")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            cookie_samesite,
            read_routing,
            import_max_documents: env::var("IMPORT_MAX_DOCUMENTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100_000),
            usage_retention_days: env::var("USAGE_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            docker_socket: PathBuf::from(
                env::var("DOCKER_SOCKET").unwrap_or_else(|_| "/var/run/docker.sock".into()),
            ),
            docker_replica_mode: match env::var("DOCKER_REPLICA_MODE")
                .unwrap_or_else(|_| "host".into())
                .as_str()
            {
                "host" => DockerReplicaMode::Host,
                "network" => DockerReplicaMode::Network,
                other => anyhow::bail!("DOCKER_REPLICA_MODE must be host or network, got {other}"),
            },
            docker_network: env::var("DOCKER_NETWORK")
                .unwrap_or_else(|_| "openwrite_default".into()),
            docker_replica_host: env::var("DOCKER_REPLICA_HOST")
                .unwrap_or_else(|_| "127.0.0.1".into()),
            docker_leader_host: env::var("DOCKER_LEADER_HOST").ok(),
            docker_leader_port: env::var("DOCKER_LEADER_PORT")
                .ok()
                .and_then(|v| v.parse().ok()),
        })
    }
}

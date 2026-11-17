//! Minimal Docker Engine client over the local Unix socket. No new dependencies:
//! the engine speaks plain HTTP/1.1, so a UnixStream and serde_json are enough.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::{Duration, timeout},
};

use crate::config::{Config, DockerReplicaMode};

pub const MANAGED_LABEL: &str = "openwrite.managed";
pub const NODE_LABEL: &str = "openwrite.node";
pub const NAME_PREFIX: &str = "ow-replica-";

/// Bootstraps an empty Postgres data dir by streaming from the current leader.
/// Passed as an env var so containers need no bind-mounted script.
const BOOTSTRAP: &str = r#"
set -eu
if [ ! -s "$PGDATA/PG_VERSION" ]; then
  echo "waiting for leader at $SOURCE_HOST:$SOURCE_PORT"
  until pg_isready -h "$SOURCE_HOST" -p "$SOURCE_PORT" -U "$SOURCE_USER" >/dev/null 2>&1; do sleep 1; done
  mkdir -p "$PGDATA"
  chown -R postgres:postgres "$PGDATA"
  gosu postgres pg_basebackup -h "$SOURCE_HOST" -p "$SOURCE_PORT" -U "$SOURCE_USER" \
    -D "$PGDATA" -Fp -Xs -R -P -w
fi
exec docker-entrypoint.sh postgres
"#;

#[derive(Clone)]
pub struct Docker {
    socket: PathBuf,
    cfg: Config,
}

#[derive(Debug)]
struct Response {
    status: u16,
    body: String,
}

impl Docker {
    pub fn new(cfg: &Config) -> Self {
        Self {
            socket: cfg.docker_socket.clone(),
            cfg: cfg.clone(),
        }
    }

    async fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Response> {
        if !self.socket.exists() {
            bail!("Docker socket {} not found", self.socket.display());
        }
        let payload = body.map(serde_json::to_vec).transpose()?.unwrap_or_default();
        let mut stream = UnixStream::connect(&self.socket)
            .await
            .with_context(|| format!("could not connect to {}", self.socket.display()))?;
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: docker\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        stream.write_all(request.as_bytes()).await?;
        if !payload.is_empty() {
            stream.write_all(&payload).await?;
        }
        stream.flush().await?;

        let mut raw = Vec::new();
        timeout(Duration::from_secs(30), stream.read_to_end(&mut raw))
            .await
            .map_err(|_| anyhow!("Docker did not respond in time"))??;
        let text = String::from_utf8_lossy(&raw);
        let (head, body) = text.split_once("\r\n\r\n").context("invalid Docker response")?;
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .context("invalid Docker status line")?;
        let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
            decode_chunked(body)?
        } else {
            body.to_string()
        };
        Ok(Response { status, body })
    }

    async fn ok(&self, method: &str, path: &str, body: Option<&Value>) -> Result<String> {
        let response = self.request(method, path, body).await?;
        if !(200..300).contains(&response.status) {
            bail!("{}", docker_message(&response.body, response.status));
        }
        Ok(response.body)
    }

    pub async fn version(&self) -> Result<String> {
        let body = self.ok("GET", "/version", None).await?;
        let value: Value = serde_json::from_str(&body).context("invalid Docker /version response")?;
        Ok(value
            .get("Version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string())
    }

    async fn create(&self, name: &str, spec: &Value) -> Result<String> {
        let body = self
            .ok(
                "POST",
                &format!("/containers/create?name={name}"),
                Some(spec),
            )
            .await?;
        let value: Value = serde_json::from_str(&body).context("invalid create response")?;
        value
            .get("Id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("Docker did not return a container ID")
    }

    async fn start(&self, id: &str) -> Result<()> {
        self.ok("POST", &format!("/containers/{id}/start"), None)
            .await?;
        Ok(())
    }

    async fn remove(&self, id: &str) -> Result<()> {
        self.ok("DELETE", &format!("/containers/{id}?force=1"), None)
            .await?;
        Ok(())
    }

    async fn inspect(&self, id: &str) -> Result<Value> {
        let body = self.ok("GET", &format!("/containers/{id}/json"), None).await?;
        serde_json::from_str(&body).context("invalid inspect response")
    }

    /// Container ID for a node registered by the replica spawner, if still present.
    pub async fn container_for_node(&self, node_name: &str) -> Result<Option<String>> {
        let filter = json!({ "label": [format!("{NODE_LABEL}={node_name}")] });
        let path = format!(
            "/containers/json?all=1&filters={}",
            urlencoded(&filter.to_string())
        );
        let body = self.ok("GET", &path, None).await?;
        let list: Value = serde_json::from_str(&body).context("invalid container list")?;
        Ok(list
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("Id"))
            .and_then(Value::as_str)
            .map(str::to_string))
    }

    /// Spawn a replica container, returning (container id, connection DSN).
    pub async fn spawn_replica(&self, leader_dsn: &str, name: &str) -> Result<(String, String)> {
        let leader = dsn_parts(leader_dsn)?;
        let leader_host = self
            .cfg
            .docker_leader_host
            .clone()
            .unwrap_or_else(|| match leader.host.as_str() {
                "127.0.0.1" | "localhost" | "::1" => "host.docker.internal".into(),
                other => other.to_string(),
            });

        let mut host_config = json!({
            "RestartPolicy": { "Name": "unless-stopped" },
            "ExtraHosts": ["host.docker.internal:host-gateway"],
        });
        // A user-defined network gives replicas Docker DNS for the leader name.
        host_config["NetworkMode"] = json!(self.cfg.docker_network);
        if self.cfg.docker_replica_mode == DockerReplicaMode::Host {
            host_config["PortBindings"] = json!({
                "5432/tcp": [{ "HostIp": "127.0.0.1", "HostPort": "" }]
            });
        }

        let spec = json!({
            "Image": "postgres:17-alpine",
            "Entrypoint": ["/bin/sh", "-c", BOOTSTRAP],
            "Env": [
                format!("POSTGRES_USER={}", leader.user),
                format!("POSTGRES_PASSWORD={}", leader.password),
                format!("POSTGRES_DB={}", leader.database),
                "PGDATA=/var/lib/postgresql/data/pgdata".to_string(),
                format!("PGPASSWORD={}", leader.password),
                format!("SOURCE_HOST={leader_host}"),
                format!(
                    "SOURCE_PORT={}",
                    self.cfg.docker_leader_port.unwrap_or(leader.port)
                ),
                format!("SOURCE_USER={}", leader.user),
            ],
            "ExposedPorts": { "5432/tcp": {} },
            "HostConfig": host_config,
            "Labels": {
                MANAGED_LABEL: "true",
                NODE_LABEL: name,
            },
        });
        let id = self.create(name, &spec).await?;
        if let Err(err) = self.start(&id).await {
            let _ = self.remove(&id).await;
            return Err(err);
        }

        let inspect = match self.inspect(&id).await {
            Ok(value) => value,
            Err(err) => {
                let _ = self.remove(&id).await;
                return Err(err);
            }
        };
        let (host, port) = if self.cfg.docker_replica_mode == DockerReplicaMode::Network {
            (name.to_string(), 5432u16)
        } else {
            let published = inspect
                .pointer("/NetworkSettings/Ports/5432~1tcp/0/HostPort")
                .and_then(Value::as_str)
                .context("Docker did not publish a replica port")?;
            let port = published
                .parse::<u16>()
                .context("invalid replica port from Docker")?;
            (self.cfg.docker_replica_host.clone(), port)
        };
        let mut dsn = url::Url::parse("postgres://localhost/openwrite")?;
        let _ = dsn.set_username(&leader.user);
        let _ = dsn.set_password(Some(&leader.password));
        dsn.set_host(Some(&host))?;
        let _ = dsn.set_port(Some(port));
        dsn.set_path(&format!("/{}", leader.database));
        Ok((id, dsn.to_string()))
    }

    pub async fn drop_container(&self, id: &str) -> Result<()> {
        self.remove(id).await
    }
}

struct DsnParts {
    user: String,
    password: String,
    host: String,
    port: u16,
    database: String,
}

fn dsn_parts(dsn: &str) -> Result<DsnParts> {
    let url = url::Url::parse(dsn).context("invalid leader DSN")?;
    if url.scheme() != "postgres" && url.scheme() != "postgresql" {
        bail!("leader DSN must use postgres://");
    }
    Ok(DsnParts {
        user: percent_decode(url.username()),
        password: url.password().map(percent_decode).unwrap_or_default(),
        host: url.host_str().context("leader DSN has no host")?.to_string(),
        port: url.port().unwrap_or(5432),
        database: url.path().trim_start_matches('/').to_string(),
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn decode_chunked(body: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = body;
    loop {
        let (size_line, tail) = rest.split_once("\r\n").context("invalid chunk header")?;
        let size = usize::from_str_radix(size_line.trim(), 16).context("invalid chunk size")?;
        if size == 0 {
            break;
        }
        let (chunk, tail) = tail.split_at_checked(size).context("truncated chunk")?;
        out.push_str(chunk);
        rest = tail.strip_prefix("\r\n").context("invalid chunk terminator")?;
    }
    Ok(out)
}

fn docker_message(body: &str, status: u16) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("Docker returned HTTP {status}: {body}"))
}

fn urlencoded(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_leader_dsn() {
        let parts = dsn_parts("postgres://ow:p%40ss@127.0.0.1:5433/openwrite").unwrap();
        assert_eq!(parts.user, "ow");
        assert_eq!(parts.password, "p@ss");
        assert_eq!(parts.host, "127.0.0.1");
        assert_eq!(parts.port, 5433);
        assert_eq!(parts.database, "openwrite");
    }

    #[test]
    fn decodes_chunked_docker_body() {
        assert_eq!(decode_chunked("5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n").unwrap(), "hello world");
    }

    #[test]
    fn urlencodes_filters() {
        assert_eq!(urlencoded("{\"a\":1}"), "%7B%22a%22%3A1%7D");
    }

    #[test]
    fn surfaces_docker_error_messages() {
        assert_eq!(docker_message("{\"message\":\"no space\"}", 500), "no space");
    }
}

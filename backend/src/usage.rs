use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::{
    Json,
    extract::{ConnectInfo, MatchedPath, Request, State},
    http::{HeaderValue, Method, header},
    middleware::Next,
    response::Response,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use url::Url;

use crate::{auth::AppState, error::ApiResult};

#[derive(sqlx::FromRow)]
struct BucketRow {
    bucket: chrono::DateTime<chrono::Utc>,
    requests: i64,
    errors: i64,
    signups: i64,
}

fn header_str(value: Option<&HeaderValue>) -> Option<&str> {
    value
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Reduce an Origin or Referer header to its host, keeping the port for local hosts.
fn origin_host(value: Option<&HeaderValue>) -> String {
    let Some(raw) = header_str(value) else {
        return String::new();
    };
    let Ok(parsed) = Url::parse(raw) else {
        return String::new();
    };
    let Some(host) = parsed.host_str() else {
        return String::new();
    };
    let local = host.starts_with("localhost") || host.starts_with("127.") || host == "[::1]";
    match parsed.port() {
        Some(port) if local => format!("{host}:{port}"),
        _ => host.to_string(),
    }
}

/// The project a request belongs to: the Appwrite project header, or the
/// `a_session_<projectId>` cookie on user-session routes.
fn request_project(req: &Request) -> String {
    if let Some(project) = header_str(req.headers().get("x-appwrite-project")) {
        return project.to_string();
    }
    if let Some(cookies) = header_str(req.headers().get(header::COOKIE)) {
        for pair in cookies.split(';') {
            let name = pair.trim().split('=').next().unwrap_or("");
            if let Some(project) = name.strip_prefix("a_session_")
                && !project.is_empty()
            {
                return project.to_string();
            }
        }
    }
    String::new()
}

fn request_client(req: &Request) -> String {
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string())
        .unwrap_or_default()
}

fn request_route(req: &Request, fallback: &str) -> String {
    if let Some(matched) = req.extensions().get::<MatchedPath>() {
        return matched.as_str().to_string();
    }
    fallback.to_string()
}

/// Record every /v1 request as one event row. Recording never fails a request.
pub async fn record(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let started = Instant::now();
    let path = req.uri().path().to_string();
    let skip = req.method() == Method::OPTIONS || path.starts_with("/v1/health");
    let method = req.method().to_string();
    let project = request_project(&req);
    let origin_from_header = origin_host(req.headers().get(header::ORIGIN));
    let origin = if origin_from_header.is_empty() {
        origin_host(req.headers().get(header::REFERER))
    } else {
        origin_from_header
    };
    let user_agent = header_str(req.headers().get(header::USER_AGENT))
        .map(|ua| ua.chars().take(180).collect::<String>())
        .unwrap_or_default();
    let client = request_client(&req);
    let route = request_route(&req, &path);

    let response = next.run(req).await;

    if !skip {
        let duration_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
        let status = response.status().as_u16() as i32;
        let pool = state.writer().await;
        tokio::spawn(async move {
            let result = sqlx::query(
                "insert into ow_request_events (project_id, method, route, status, duration_ms, origin, client, user_agent) \
                 values ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(&project)
            .bind(&method)
            .bind(&route)
            .bind(status)
            .bind(duration_ms as i32)
            .bind(&origin)
            .bind(&client)
            .bind(&user_agent)
            .execute(&pool)
            .await;
            if let Err(err) = result {
                tracing::debug!("could not record request event: {err}");
            }
        });
    }

    response
}

pub async fn retention_loop(pool: PgPool, days: u32) {
    let mut ticker = tokio::time::interval(Duration::from_secs(6 * 3600));
    loop {
        ticker.tick().await;
        match sqlx::query("delete from ow_request_events where created_at < now() - make_interval(days => $1)")
            .bind(days as i32)
            .execute(&pool)
            .await
        {
            Ok(result) if result.rows_affected() > 0 => {
                tracing::info!("pruned {} request events older than {days} days", result.rows_affected());
            }
            Ok(_) => {}
            Err(err) => tracing::warn!("usage retention cleanup failed: {err}"),
        }
    }
}

const SIGNUP_FILTER: &str =
    "count(*) filter (where route = '/v1/account' and method = 'POST' and status < 300)";

fn top_query(where_clause: &str, group: &str) -> String {
    format!(
        "select {group} as key, count(*)::bigint as requests {where_clause} \
         and created_at > now() - make_interval(days => $1) \
         group by 1 order by 2 desc limit 8"
    )
}

fn truncate_to(dt: chrono::DateTime<chrono::Utc>, hourly: bool) -> chrono::DateTime<chrono::Utc> {
    use chrono::Timelike;
    let dt = dt.with_nanosecond(0).unwrap_or(dt);
    let dt = dt.with_second(0).unwrap_or(dt);
    let dt = dt.with_minute(0).unwrap_or(dt);
    if hourly {
        dt
    } else {
        dt.with_hour(0).unwrap_or(dt)
    }
}

/// Aggregated usage for the console: timeseries, totals and top lists.
pub async fn aggregate(state: AppState, days: u32) -> ApiResult<Json<Value>> {
    let days = days.clamp(1, 90);
    let hourly = days <= 7;
    let bucket_unit = if hourly { "hour" } else { "day" };
    let pool = state.writer().await;

    let rows: Vec<BucketRow> = sqlx::query_as::<_, BucketRow>(&format!(
        "select date_trunc('{bucket_unit}', created_at) as bucket, \
                count(*)::bigint as requests, \
                count(*) filter (where status >= 400)::bigint as errors, \
                {SIGNUP_FILTER} as signups \
         from ow_request_events \
         where created_at > now() - make_interval(days => $1) \
         group by 1 order by 1"
    ))
    .bind(days as i32)
    .fetch_all(&pool)
    .await?;

    let mut by_bucket = std::collections::HashMap::new();
    for row in rows {
        by_bucket.insert(row.bucket, (row.requests, row.errors, row.signups));
    }
    let now = chrono::Utc::now();
    let start = truncate_to(now - chrono::Duration::hours((days as i64) * 24), hourly);
    let step = if hourly {
        chrono::Duration::hours(1)
    } else {
        chrono::Duration::days(1)
    };
    let mut timeseries = Vec::new();
    let mut bucket = start;
    let horizon = now + step;
    while bucket < horizon {
        let (requests, errors, signups) =
            by_bucket.get(&bucket).copied().unwrap_or((0, 0, 0));
        timeseries.push(json!({
            "bucket": bucket.to_rfc3339(),
            "requests": requests,
            "errors": errors,
            "signups": signups,
        }));
        bucket += step;
    }

    let totals: (i64, i64, f64, i64, i64) = sqlx::query_as(
        "select count(*)::bigint, \
                count(*) filter (where status >= 400)::bigint, \
                coalesce(avg(duration_ms), 0)::float8, \
                count(distinct project_id) filter (where project_id <> '')::bigint, \
                count(distinct origin) filter (where origin <> '')::bigint \
         from ow_request_events \
         where created_at > now() - make_interval(days => $1)",
    )
    .bind(days as i32)
    .fetch_one(&pool)
    .await?;

    let projects: Vec<(String, i64)> = sqlx::query_as(&top_query(
        "from ow_request_events where project_id <> ''",
        "project_id",
    ))
    .bind(days as i32)
    .fetch_all(&pool)
    .await?;

    let routes: Vec<(String, String, i64)> = sqlx::query_as(
        "select route, method, count(*)::bigint as requests \
         from ow_request_events \
         where created_at > now() - make_interval(days => $1) \
         group by 1, 2 order by 3 desc limit 8",
    )
    .bind(days as i32)
    .fetch_all(&pool)
    .await?;

    let origins: Vec<(String, i64, i64)> = sqlx::query_as(&format!(
        "select origin, count(*)::bigint as requests, {SIGNUP_FILTER} as signups \
         from ow_request_events \
         where origin <> '' and created_at > now() - make_interval(days => $1) \
         group by 1 order by 2 desc limit 8"
    ))
    .bind(days as i32)
    .fetch_all(&pool)
    .await?;

    let agents: Vec<(String, i64)> = sqlx::query_as(&top_query(
        "from ow_request_events where user_agent <> ''",
        "user_agent",
    ))
    .bind(days as i32)
    .fetch_all(&pool)
    .await?;

    Ok(Json(json!({
        "hourly": hourly,
        "days": days,
        "timeseries": timeseries,
        "totals": {
            "requests": totals.0,
            "errors": totals.1,
            "avgLatencyMs": totals.2.round() as i64,
            "projects": totals.3,
            "origins": totals.4,
        },
        "projects": projects.into_iter().map(|(project, requests)| json!({
            "project": project,
            "requests": requests,
        })).collect::<Vec<_>>(),
        "routes": routes.into_iter().map(|(route, method, requests)| json!({
            "route": route,
            "method": method,
            "requests": requests,
        })).collect::<Vec<_>>(),
        "origins": origins.into_iter().map(|(origin, requests, signups)| json!({
            "origin": origin,
            "requests": requests,
            "signups": signups,
        })).collect::<Vec<_>>(),
        "agents": agents.into_iter().map(|(agent, requests)| json!({
            "agent": agent,
            "requests": requests,
        })).collect::<Vec<_>>(),
    })))
}

use std::{cmp::Ordering, time::Duration};

use axum::{
    extract::{
        RawQuery, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::Response,
};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use url::form_urlencoded;

use crate::{
    auth::{self, AppState, request_session_token},
    documents::{self, DocAccess},
    error::{ApiError, ApiResult},
    models::CollectionRow,
    store,
};

fn hub() -> &'static broadcast::Sender<String> {
    static HUB: std::sync::OnceLock<broadcast::Sender<String>> = std::sync::OnceLock::new();
    HUB.get_or_init(|| broadcast::channel(256).0)
}

/// Follow the Postgres NOTIFY events emitted by the document write paths and
/// fan them out to connected realtime sockets.
pub async fn listener_task(pool: sqlx::PgPool) {
    loop {
        match sqlx::postgres::PgListener::connect_with(&pool).await {
            Ok(mut listener) => {
                if let Err(err) = listener.listen("ow_docs").await {
                    tracing::warn!("realtime listener could not subscribe to ow_docs: {err}");
                } else {
                    tracing::info!("realtime is listening for document changes");
                    loop {
                        match listener.recv().await {
                            Ok(notification) => {
                                let _ = hub().send(notification.payload().to_string());
                            }
                            Err(err) => {
                                tracing::warn!("realtime listener failed: {err}");
                                break;
                            }
                        }
                    }
                }
            }
            Err(err) => tracing::warn!("realtime listener could not connect: {err}"),
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

struct ConnectParams {
    project: String,
    channels: Vec<String>,
    token: Option<String>,
}

fn parse_query(raw: &str) -> ConnectParams {
    let mut project = String::new();
    let mut channels = Vec::new();
    let mut token = None;
    for (key, value) in form_urlencoded::parse(raw.as_bytes()) {
        match key.as_ref() {
            "project" => project = value.into_owned(),
            // Legacy SDKs put channels in the query string; modern ones send a
            // subscribe message after the handshake.
            "channels[]" | "channels" => channels.push(value.into_owned()),
            "jwt" | "token" | "session" => token = Some(value.into_owned()),
            _ => {}
        }
    }
    ConnectParams {
        project,
        channels,
        token,
    }
}

#[derive(Clone, Debug)]
struct Subscription {
    id: String,
    channels: Vec<String>,
    queries: Vec<String>,
}

fn split_action(channel: &str) -> (&str, Option<&str>) {
    for action in ["create", "update", "delete", "upsert"] {
        if let Some(base) = channel.strip_suffix(&format!(".{action}")) {
            return (base, Some(action));
        }
    }
    (channel, None)
}

/// Match a subscription channel against one of the concrete channels an event
/// belongs to. `*` matches a whole dotted segment, as in Appwrite channels.
fn channel_matches(pattern: &str, channel: &str, action: &str) -> bool {
    let (pattern, wanted_action) = split_action(pattern);
    if wanted_action.is_some_and(|wanted| wanted != action) {
        return false;
    }
    let pattern: Vec<&str> = pattern.split('.').collect();
    let channel: Vec<&str> = channel.split('.').collect();
    pattern.len() == channel.len()
        && pattern
            .iter()
            .zip(&channel)
            .all(|(p, c)| *p == "*" || p == c)
}

/// The channels every document event belongs to, short legacy names first.
fn event_channels(database: &str, collection: &str, document: &str) -> Vec<String> {
    vec![
        "documents".to_string(),
        format!("documents.{collection}"),
        format!("documents.{collection}.{document}"),
        "rows".to_string(),
        format!("rows.{collection}"),
        format!("rows.{collection}.{document}"),
        format!("databases.{database}.collections.{collection}.documents"),
        format!("databases.{database}.collections.{collection}.documents.{document}"),
        format!("databases.{database}.tables.{collection}.rows"),
        format!("databases.{database}.tables.{collection}.rows.{document}"),
        format!("tablesdb.{database}.tables.{collection}.rows"),
        format!("tablesdb.{database}.tables.{collection}.rows.{document}"),
    ]
}

/// Events use the modern tablesdb prefix plus the legacy databases/documents
/// name so both old and new SDKs can match the same notification.
fn event_names(database: &str, collection: &str, document: &str, action: &str) -> Vec<String> {
    vec![
        format!("tablesdb.{database}.tables.{collection}.rows.{document}.{action}"),
        format!("databases.{database}.tables.{collection}.rows.{document}.{action}"),
        format!("databases.{database}.collections.{collection}.documents.{document}.{action}"),
    ]
}

fn readable(collection: &CollectionRow, payload: &Value, user_id: Option<&str>) -> bool {
    let permissions: Vec<String> = if collection.document_security {
        payload
            .get("$permissions")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    } else {
        serde_json::from_value(collection.permissions.clone()).unwrap_or_default()
    };
    crate::schema::permission_allows(&permissions, "read", user_id)
}

fn field<'a>(payload: &'a Value, attribute: &str) -> Option<&'a Value> {
    let mut current = payload;
    for part in attribute.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn compare(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => a.as_f64()?.partial_cmp(&b.as_f64()?),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

fn values_equal(left: &Value, right: &Value) -> bool {
    left == right || compare(left, right) == Some(Ordering::Equal)
}

fn query_matches(raw: &str, payload: &Value) -> bool {
    let Ok(query) = serde_json::from_str::<Value>(raw) else {
        // Unknown query formats must not hide events from the subscriber.
        return true;
    };
    let method = query.get("method").and_then(Value::as_str).unwrap_or("");
    let attribute = query.get("attribute").and_then(Value::as_str).unwrap_or("");
    let values = match query.get("values") {
        Some(Value::Array(values)) => values.clone(),
        Some(value) => vec![value.clone()],
        None => Vec::new(),
    };
    let value = field(payload, attribute);
    match method {
        "equal" => value.is_some_and(|value| values.iter().any(|want| values_equal(value, want))),
        "notEqual" => value.is_none_or(|value| {
            values.iter().all(|want| !values_equal(value, want))
        }),
        "lessThan" => value
            .and_then(|value| values.first().and_then(|want| compare(value, want)))
            .is_some_and(|order| order == Ordering::Less),
        "lessThanEqual" => value
            .and_then(|value| values.first().and_then(|want| compare(value, want)))
            .is_some_and(|order| order != Ordering::Greater),
        "greaterThan" => value
            .and_then(|value| values.first().and_then(|want| compare(value, want)))
            .is_some_and(|order| order == Ordering::Greater),
        "greaterThanEqual" => value
            .and_then(|value| values.first().and_then(|want| compare(value, want)))
            .is_some_and(|order| order != Ordering::Less),
        "isNull" => value.is_none_or(Value::is_null),
        "isNotNull" => value.is_some_and(|value| !value.is_null()),
        "contains" => value.is_some_and(|value| match value {
            Value::Array(items) => {
                items.iter().any(|item| values.iter().any(|want| values_equal(item, want)))
            }
            Value::String(text) => values
                .iter()
                .filter_map(Value::as_str)
                .any(|want| text.contains(want)),
            _ => false,
        }),
        "search" => value
            .and_then(Value::as_str)
            .is_some_and(|text| match values.first().and_then(Value::as_str) {
                Some(want) => text.to_lowercase().contains(&want.to_lowercase()),
                None => false,
            }),
        "and" | "or" => {
            let matches = |raw: &Value| {
                raw.as_str()
                    .is_some_and(|raw| query_matches(raw, payload))
            };
            if method == "and" {
                values.iter().all(matches)
            } else {
                values.iter().any(matches)
            }
        }
        // Pagination, ordering and unknown methods do not filter realtime events.
        _ => true,
    }
}

fn queries_match(queries: &[String], payload: &Value) -> bool {
    queries.iter().all(|query| query_matches(query, payload))
}

fn subscription_from(value: &Value, fallback_id: &mut u64) -> Option<Subscription> {
    let channels = value
        .get("channels")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if channels.is_empty() {
        return None;
    }
    let id = match value.get("subscriptionId").and_then(Value::as_str) {
        Some(id) => id.to_string(),
        None => {
            *fallback_id += 1;
            format!("ow-{fallback_id}")
        }
    };
    Some(Subscription {
        id,
        channels,
        queries: value
            .get("queries")
            .and_then(Value::as_array)
            .map(|queries| {
                queries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn subscribe_rows(data: &Value, fallback_id: &mut u64) -> Vec<Subscription> {
    let rows = match data {
        Value::Array(rows) => rows.iter().collect::<Vec<_>>(),
        Value::Object(_) => vec![data],
        _ => Vec::new(),
    };
    rows.into_iter()
        .filter_map(|row| subscription_from(row, fallback_id))
        .collect()
}

fn unsubscribe_ids(data: &Value) -> Vec<String> {
    let rows = match data {
        Value::Array(rows) => rows.iter().collect::<Vec<_>>(),
        _ => vec![data],
    };
    rows.into_iter()
        .map(|row| match row {
            Value::String(id) => id.clone(),
            other => other
                .get("subscriptionId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })
        .filter(|id| !id.is_empty())
        .collect()
}

fn auth_token(data: &Value) -> Option<&str> {
    ["session", "jwt", "token"]
        .iter()
        .find_map(|key| data.get(*key).and_then(Value::as_str))
}

pub async fn realtime(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
    RawQuery(raw): RawQuery,
) -> ApiResult<Response> {
    let params = parse_query(raw.as_deref().unwrap_or(""));
    if params.project.is_empty() {
        return Err(ApiError::bad_request("The project query parameter is required"));
    }
    {
        let pool = state.writer().await;
        if store::find_project(&pool, &params.project).await?.is_none() {
            return Err(ApiError::not_found("Project not found"));
        }
    }
    // Session cookie first, then the token the SDK fetched from /v1/account/jwts
    // (browsers may withhold cookies on cross-origin WebSocket upgrades).
    let mut token = request_session_token(&headers, &params.project);
    if token.is_none() {
        token = params.token;
    }
    let session = match token {
        Some(token) => auth::session_lookup(&state, &params.project, &token)
            .await
            .unwrap_or(None),
        None => None,
    };
    let user_id = session.map(|session| session.user_id);

    Ok(upgrade.on_upgrade(move |socket| {
        handle_socket(state, socket, params.project, params.channels, user_id)
    }))
}

async fn user_value(state: &AppState, project_id: &str, user_id: Option<&str>) -> Value {
    let Some(user_id) = user_id else {
        return Value::Null;
    };
    let pool = state.writer().await;
    match store::find_user(&pool, project_id, user_id).await {
        Ok(Some(user)) => crate::models::user_json(&user),
        _ => Value::Null,
    }
}

async fn send_json(socket: &mut WebSocket, value: &Value) -> bool {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .is_ok()
}

async fn send_connected(socket: &mut WebSocket, user: Value) -> bool {
    send_json(
        socket,
        &json!({
            "type": "connected",
            "data": { "channels": [], "user": user },
        }),
    )
    .await
}

async fn handle_socket(
    state: AppState,
    mut socket: WebSocket,
    project_id: String,
    query_channels: Vec<String>,
    mut user_id: Option<String>,
) {
    let user = user_value(&state, &project_id, user_id.as_deref()).await;
    if !send_connected(&mut socket, user).await {
        return;
    }

    let mut subscriptions = Vec::new();
    if !query_channels.is_empty() {
        subscriptions.push(Subscription {
            id: "ow-0".to_string(),
            channels: query_channels,
            queries: Vec::new(),
        });
    }
    let mut changes = hub().subscribe();
    let mut fallback_id = 0;

    loop {
        tokio::select! {
            result = changes.recv() => {
                let payload = match result {
                    Ok(payload) => payload,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                };
                let Ok(change) = serde_json::from_str::<Value>(&payload) else {
                    continue;
                };
                let (Some(database), Some(collection_id), Some(document), Some(action)) = (
                    change.get("database").and_then(Value::as_str),
                    change.get("collection").and_then(Value::as_str),
                    change.get("document").and_then(Value::as_str),
                    change.get("event").and_then(Value::as_str),
                ) else {
                    continue;
                };
                if action != "create" && action != "update" && action != "delete" {
                    continue;
                }
                if change.get("project").and_then(Value::as_str) != Some(project_id.as_str()) {
                    continue;
                }
                let channels = event_channels(database, collection_id, document);
                let mut matched_channels: Vec<String> = Vec::new();
                let mut matched_subscriptions: Vec<String> = Vec::new();
                for subscription in &subscriptions {
                    if !subscription.channels.iter().any(|pattern| {
                        channels
                            .iter()
                            .any(|channel| channel_matches(pattern, channel, action))
                    }) {
                        continue;
                    }
                    matched_subscriptions.push(subscription.id.clone());
                }
                if matched_subscriptions.is_empty() {
                    continue;
                }

                let pool = state.writer().await;
                let Ok(collection) =
                    store::get_collection(&pool, &project_id, database, collection_id).await
                else {
                    continue;
                };
                let embedded = change.get("payload").filter(|value| !value.is_null()).cloned();
                let payload = match embedded {
                    Some(payload) if readable(&collection, &payload, user_id.as_deref()) => payload,
                    Some(_) => continue,
                    None if action == "delete" => {
                        let permissions = change
                            .get("permissions")
                            .cloned()
                            .unwrap_or(Value::Array(Vec::new()));
                        let payload = json!({
                            "$id": document,
                            "$databaseId": database,
                            "$collectionId": collection_id,
                            "$permissions": permissions,
                        });
                        if !readable(&collection, &payload, user_id.as_deref()) {
                            continue;
                        }
                        payload
                    }
                    None => {
                        let access = DocAccess {
                            is_api_key: false,
                            user_id: user_id.as_deref(),
                        };
                        let Ok(payload) =
                            documents::get_document(&pool, access, &collection, document).await
                        else {
                            continue;
                        };
                        payload
                    }
                };
                // Queries are evaluated on the final payload for this subscriber.
                matched_subscriptions.retain(|id| {
                    subscriptions
                        .iter()
                        .find(|subscription| &subscription.id == id)
                        .is_some_and(|subscription| {
                            queries_match(&subscription.queries, &payload)
                        })
                });
                if matched_subscriptions.is_empty() {
                    continue;
                }
                for subscription in &subscriptions {
                    if !matched_subscriptions.contains(&subscription.id) {
                        continue;
                    }
                    for channel in &channels {
                        if subscription
                            .channels
                            .iter()
                            .any(|pattern| channel_matches(pattern, channel, action))
                            && !matched_channels.contains(channel)
                        {
                            matched_channels.push(channel.clone());
                        }
                    }
                }

                let message = json!({
                    "type": "event",
                    "data": {
                        "events": event_names(database, collection_id, document, action),
                        "channels": matched_channels,
                        "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        "payload": payload,
                        "subscriptions": matched_subscriptions,
                    },
                });
                if !send_json(&mut socket, &message).await {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if text == "ping" {
                            if socket.send(Message::Text("pong".into())).await.is_err() {
                                break;
                            }
                            continue;
                        }
                        let Ok(value) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        match value.get("type").and_then(Value::as_str) {
                            Some("ping") => {
                                if !send_json(&mut socket, &json!({"type": "pong"})).await {
                                    break;
                                }
                            }
                            Some("subscribe") => {
                                for subscription in subscribe_rows(
                                    value.get("data").unwrap_or(&Value::Null),
                                    &mut fallback_id,
                                ) {
                                    if let Some(existing) = subscriptions
                                        .iter_mut()
                                        .find(|existing| existing.id == subscription.id)
                                    {
                                        *existing = subscription;
                                    } else {
                                        subscriptions.push(subscription);
                                    }
                                }
                            }
                            Some("unsubscribe") => {
                                let ids = unsubscribe_ids(value.get("data").unwrap_or(&Value::Null));
                                subscriptions.retain(|subscription| !ids.contains(&subscription.id));
                            }
                            Some("authentication") => {
                                let Some(token) = value
                                    .get("data")
                                    .and_then(auth_token)
                                else {
                                    continue;
                                };
                                match auth::session_lookup(&state, &project_id, token).await {
                                    Ok(Some(session)) => {
                                        user_id = Some(session.user_id);
                                        let user = user_value(
                                            &state,
                                            &project_id,
                                            user_id.as_deref(),
                                        )
                                        .await;
                                        if !send_connected(&mut socket, user).await {
                                            break;
                                        }
                                    }
                                    _ => {
                                        if !send_json(
                                            &mut socket,
                                            &json!({
                                                "type": "error",
                                                "data": {
                                                    "message": "Invalid session",
                                                    "code": 401,
                                                },
                                            }),
                                        )
                                        .await
                                        {
                                            break;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channels() -> Vec<String> {
        event_channels("main", "notes", "doc1")
    }

    #[test]
    fn legacy_and_database_channels_match() {
        assert!(channel_matches("documents", "documents", "create"));
        assert!(channel_matches("documents.notes", "documents.notes", "update"));
        assert!(channel_matches(
            "documents.notes.doc1",
            "documents.notes.doc1",
            "delete"
        ));
        assert!(channel_matches(
            "databases.main.collections.notes.documents",
            "databases.main.collections.notes.documents",
            "create"
        ));
        assert!(channel_matches(
            "databases.main.collections.notes.documents.doc1",
            "databases.main.collections.notes.documents.doc1",
            "create"
        ));
    }

    #[test]
    fn wildcards_match_whole_segments() {
        assert!(channel_matches(
            "databases.*.collections.notes.documents",
            "databases.main.collections.notes.documents",
            "create"
        ));
        assert!(channel_matches("documents.*", "documents.notes", "update"));
        assert!(!channel_matches("documents.*", "documents.notes.doc1", "update"));
        assert!(!channel_matches(
            "databases.main.collections.other.documents",
            "databases.main.collections.notes.documents",
            "create"
        ));
    }

    #[test]
    fn action_suffix_filters_events() {
        assert!(channel_matches("documents.notes.create", "documents.notes", "create"));
        assert!(!channel_matches("documents.notes.create", "documents.notes", "update"));
        assert!(!channel_matches("documents.notes.*", "documents.notes", "update"));
    }

    #[test]
    fn event_channels_cover_short_and_long_names() {
        let channels = channels();
        assert!(channels.contains(&"documents".to_string()));
        assert!(channels.contains(&"documents.notes".to_string()));
        assert!(channels.contains(&"documents.notes.doc1".to_string()));
        assert!(channels.contains(&"rows".to_string()));
        assert!(channels.contains(&"databases.main.collections.notes.documents".to_string()));
        assert!(channels.contains(&"databases.main.collections.notes.documents.doc1".to_string()));
        assert!(channels.contains(&"databases.main.tables.notes.rows".to_string()));
        assert!(channels.contains(&"tablesdb.main.tables.notes.rows.doc1".to_string()));
    }

    #[test]
    fn event_names_cover_rows_and_documents() {
        let events = event_names("main", "notes", "doc1", "update");
        assert!(events.contains(&"tablesdb.main.tables.notes.rows.doc1.update".to_string()));
        assert!(events.contains(&"databases.main.tables.notes.rows.doc1.update".to_string()));
        assert!(
            events.contains(&"databases.main.collections.notes.documents.doc1.update".to_string())
        );
    }

    #[test]
    fn queries_filter_on_document_data() {
        let payload = json!({"title": "Hello", "votes": 3, "tags": ["a", "b"]});
        let equal = r#"{"method":"equal","attribute":"votes","values":[3]}"#;
        let less = r#"{"method":"lessThan","attribute":"votes","values":[5]}"#;
        let not_equal = r#"{"method":"lessThan","attribute":"votes","values":[1]}"#;
        assert!(query_matches(equal, &payload));
        assert!(query_matches(less, &payload));
        assert!(!query_matches(not_equal, &payload));
        assert!(query_matches(
            r#"{"method":"isNull","attribute":"missing"}"#,
            &payload
        ));
        assert!(query_matches(
            r#"{"method":"contains","attribute":"tags","values":["b"]}"#,
            &payload
        ));
    }

    #[test]
    fn unsupported_queries_do_not_hide_events() {
        assert!(query_matches("equal(\"title\", [\"x\"])", &json!({})));
        assert!(query_matches(
            r#"{"method":"orderAsc","attribute":"title","values":[]}"#,
            &json!({"title": "x"})
        ));
    }
}

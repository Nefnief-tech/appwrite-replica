use axum::{Json, extract::State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    auth::{AdminAuth, AppState},
    error::{ApiError, ApiResult},
    models::Attribute,
    store,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportBody {
    pub endpoint: String,
    pub api_key: String,
    pub remote_project_id: String,
    pub target_project_id: Option<String>,
    pub target_project_name: Option<String>,
}

struct Remote<'a> {
    http: &'a reqwest::Client,
    base: String,
    project: String,
    key: String,
}

impl Remote<'_> {
    async fn get(&self, path: &str) -> ApiResult<Value> {
        let url = format!("{}/v1{}", self.base, path);
        let response = self
            .http
            .get(&url)
            .header("x-appwrite-project", &self.project)
            .header("x-appwrite-key", &self.key)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|err| ApiError::bad_request(format!("Could not reach Appwrite: {err}")))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            let message = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| body.chars().take(300).collect());
            return Err(ApiError::bad_request(format!(
                "Appwrite returned {status}: {message}"
            )));
        }
        serde_json::from_str(&body)
            .map_err(|err| ApiError::bad_request(format!("Appwrite returned invalid JSON: {err}")))
    }
}

fn normalize_endpoint(raw: &str) -> ApiResult<String> {
    let url = url::Url::parse(raw.trim())
        .map_err(|_| ApiError::bad_request("Enter a valid Appwrite endpoint URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApiError::bad_request(
            "The Appwrite endpoint must use http or https",
        ));
    }
    let trimmed = raw.trim().trim_end_matches('/');
    let base = trimmed.strip_suffix("/v1").unwrap_or(trimmed);
    Ok(base.to_string())
}

fn parse_ts(value: Option<&Value>) -> DateTime<Utc> {
    value
        .and_then(Value::as_str)
        .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
        .map(|v| v.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

fn coerce_default(value: Option<&Value>, kind: &str) -> Option<Value> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    match kind {
        "integer" => value
            .as_i64()
            .map(Value::from)
            .or_else(|| value.as_str().and_then(|v| v.parse::<i64>().ok()).map(Value::from)),
        "float" => value
            .as_f64()
            .map(Value::from)
            .or_else(|| value.as_str().and_then(|v| v.parse::<f64>().ok()).map(Value::from)),
        "boolean" => value.as_bool().map(Value::from).or_else(|| {
            value.as_str().and_then(|v| match v {
                "true" | "1" => Some(Value::Bool(true)),
                "false" | "0" => Some(Value::Bool(false)),
                _ => None,
            })
        }),
        "datetime" => value
            .as_str()
            .filter(|v| DateTime::parse_from_rfc3339(v).is_ok())
            .map(|v| Value::String(v.to_string())),
        _ => value.as_str().map(|v| Value::String(v.to_string())),
    }
}

/// Maps Appwrite attributes onto the types Openwrite can validate and query.
/// Unsupported types (relationships, spatial, etc.) are skipped; their data is
/// still copied into the JSONB column by the raw document insert.
fn sanitize_attributes(remote: &Value) -> (Vec<Attribute>, usize) {
    let mut out = Vec::new();
    let mut skipped = 0usize;
    let Some(items) = remote.as_array() else {
        return (out, skipped);
    };
    for item in items {
        let Some(key) = item.get("key").and_then(Value::as_str) else {
            skipped += 1;
            continue;
        };
        let typ = item.get("type").and_then(Value::as_str).unwrap_or("");
        let format = item.get("format").and_then(Value::as_str).unwrap_or("");
        let kind = match typ {
            "string" if matches!(format, "email" | "url" | "ip") => format,
            "string" => "string",
            "integer" => "integer",
            "double" | "float" => "float",
            "boolean" => "boolean",
            "datetime" => "datetime",
            "enum" => "enum",
            _ => {
                skipped += 1;
                continue;
            }
        };
        let elements = item.get("elements").and_then(Value::as_array).map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        });
        if kind == "enum" && elements.as_ref().map(Vec::is_empty).unwrap_or(true) {
            skipped += 1;
            continue;
        }
        let required = item.get("required").and_then(Value::as_bool).unwrap_or(false);
        let array = item.get("array").and_then(Value::as_bool).unwrap_or(false);
        let size = item
            .get("size")
            .and_then(Value::as_i64)
            .filter(|v| *v >= 1 && *v <= 1_000_000);
        let default = coerce_default(item.get("default"), kind);
        let mut attr = Attribute {
            key: key.to_string(),
            kind: kind.to_string(),
            required,
            array,
            size,
            default,
            elements,
        };
        // Enum/array defaults from Appwrite can be a list; our validator only
        // checks scalars, so drop array defaults instead of failing the import.
        if array && attr.default.as_ref().map(|v| v.is_array()).unwrap_or(false) {
            attr.default = None;
        }
        out.push(attr);
    }
    (out, skipped)
}

fn document_data(document: &Value) -> Value {
    let mut data = Map::new();
    if let Some(obj) = document.as_object() {
        for (key, value) in obj {
            if !key.starts_with('$') {
                data.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(data)
}

fn permissions(document: &Value) -> Value {
    document
        .get("$permissions")
        .cloned()
        .unwrap_or_else(|| json!([]))
}

pub async fn import_project(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(body): Json<ImportBody>,
) -> ApiResult<Json<Value>> {
    if body.api_key.trim().is_empty() {
        return Err(ApiError::bad_request("An Appwrite API key is required"));
    }
    if body.remote_project_id.trim().is_empty() {
        return Err(ApiError::bad_request("An Appwrite project ID is required"));
    }
    let base = normalize_endpoint(&body.endpoint)?;
    let remote = Remote {
        http: state.http(),
        base,
        project: body.remote_project_id.trim().to_string(),
        key: body.api_key.trim().to_string(),
    };

    // Fail before writing anything if credentials or the endpoint are wrong.
    let databases = collect(&remote, "/databases", "databases").await?;

    let writer = state.writer().await;
    let (project_id, api_key) = match body.target_project_id.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(id) => {
            store::get_project(&writer, id).await?;
            (id.to_string(), None)
        }
        None => {
            let remote_id = body.remote_project_id.trim();
            let name = body
                .target_project_name
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{remote_id} (imported)"));
            let (project, key) = store::create_project(&writer, None, name).await?;
            (project.id, Some(key))
        }
    };

    let max_documents = state.cfg().import_max_documents;
    let mut stats = Stats::default();
    let mut truncated = false;

    for database in &databases {
        let database_id = database
            .get("$id")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("Appwrite returned a database without an ID"))?;
        let existing: Option<String> = sqlx::query_scalar(
            "select id from ow_databases where project_id = $1 and id = $2",
        )
        .bind(&project_id)
        .bind(database_id)
        .fetch_optional(&writer)
        .await?;
        if existing.is_none() {
            let name = database
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(database_id);
            store::create_database(&writer, &project_id, Some(database_id.to_string()), name.into())
                .await?;
            stats.databases += 1;
        }

        let collections = collect(
            &remote,
            &format!("/databases/{database_id}/collections"),
            "collections",
        )
        .await?;
        for collection in &collections {
            let collection_id = collection
                .get("$id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ApiError::bad_request("Appwrite returned a collection without an ID")
                })?;
            let exists: Option<String> = sqlx::query_scalar(
                "select id from ow_collections where project_id = $1 and database_id = $2 and id = $3",
            )
            .bind(&project_id)
            .bind(database_id)
            .bind(collection_id)
            .fetch_optional(&writer)
            .await?;
            if exists.is_none() {
                let name = collection
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(collection_id);
                let attributes = collection
                    .get("attributes")
                    .map(sanitize_attributes)
                    .unwrap_or_default();
                stats.skipped_attributes += attributes.1;
                let collection_permissions = collection
                    .get("$permissions")
                    .cloned()
                    .unwrap_or_else(|| json!([]));
                let document_security = collection
                    .get("documentSecurity")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let attributes = serde_json::to_value(&attributes.0)?;
                store::create_collection(
                    &writer,
                    &project_id,
                    database_id,
                    Some(collection_id.to_string()),
                    name.to_string(),
                    attributes,
                    Some(&collection_permissions),
                    document_security,
                )
                .await?;
                stats.collections += 1;
            }

            let mut offset = 0i64;
            loop {
                if stats.documents as u64 >= max_documents {
                    truncated = true;
                    break;
                }
                let path = format!(
                    "/databases/{database_id}/collections/{collection_id}/documents?limit=100&offset={offset}"
                );
                let page = remote.get(&path).await?;
                let documents = page
                    .get("documents")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let total = page.get("total").and_then(Value::as_i64).unwrap_or(0);
                if documents.is_empty() {
                    break;
                }
                for document in &documents {
                    if stats.documents as u64 >= max_documents {
                        truncated = true;
                        break;
                    }
                    let Some(id) = document.get("$id").and_then(Value::as_str) else {
                        stats.skipped_documents += 1;
                        continue;
                    };
                    let data = document_data(document);
                    let permissions = permissions(document);
                    let created = parse_ts(document.get("$createdAt"));
                    let updated = parse_ts(document.get("$updatedAt"));
                    let inserted = store::import_document(
                        &writer,
                        &project_id,
                        database_id,
                        collection_id,
                        id,
                        &data,
                        &permissions,
                        created,
                        updated,
                    )
                    .await?;
                    if inserted {
                        stats.documents += 1;
                    } else {
                        stats.skipped_documents += 1;
                    }
                }
                offset += documents.len() as i64;
                if offset >= total {
                    break;
                }
            }
        }
    }

    let users = collect(&remote, "/users", "users").await?;
    for user in &users {
        let Some(email) = user.get("email").and_then(Value::as_str) else {
            stats.skipped_users += 1;
            continue;
        };
        let id = user.get("$id").and_then(Value::as_str);
        let name = user.get("name").and_then(Value::as_str).unwrap_or("");
        let status = user
            .get("status")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let created = parse_ts(user.get("$createdAt"));
        // Appwrite never exposes password hashes: imported users need a reset.
        match store::import_user(&writer, &project_id, id, email, name, None, created, status).await
        {
            Ok(true) => stats.users += 1,
            Ok(false) => stats.skipped_users += 1,
            Err(_) => stats.skipped_users += 1,
        }
    }

    Ok(Json(json!({
        "projectId": project_id,
        "apiKey": api_key,
        "stats": stats,
        "truncated": truncated,
    })))
}

async fn collect(remote: &Remote<'_>, path: &str, key: &str) -> ApiResult<Vec<Value>> {
    let mut out = Vec::new();
    let mut offset = 0i64;
    loop {
        let separator = if path.contains('?') { '&' } else { '?' };
        let page = remote
            .get(&format!("{path}{separator}limit=100&offset={offset}"))
            .await?;
        let items = page
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let total = page.get("total").and_then(Value::as_i64).unwrap_or(0);
        out.extend(items.iter().cloned());
        offset += items.len() as i64;
        if items.len() < 100 || offset >= total {
            break;
        }
    }
    Ok(out)
}

#[derive(Default)]
struct Stats {
    databases: usize,
    collections: usize,
    documents: usize,
    users: usize,
    skipped_attributes: usize,
    skipped_documents: usize,
    skipped_users: usize,
}

impl serde::Serialize for Stats {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Stats", 7)?;
        state.serialize_field("databases", &self.databases)?;
        state.serialize_field("collections", &self.collections)?;
        state.serialize_field("documents", &self.documents)?;
        state.serialize_field("users", &self.users)?;
        state.serialize_field("skippedAttributes", &self.skipped_attributes)?;
        state.serialize_field("skippedDocuments", &self.skipped_documents)?;
        state.serialize_field("skippedUsers", &self.skipped_users)?;
        state.end()
    }
}

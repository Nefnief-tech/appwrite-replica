use axum::{
    Json,
    extract::{Path, RawQuery, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    auth::{ApiCtx, AppState},
    documents::{self, DocAccess},
    error::{ApiError, ApiResult},
    models::{Attribute, CollectionRow, attribute_json},
    store,
};

use super::documents::query_pairs;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/tablesdb/{database_id}/tables",
            get(list_tables).post(create_table),
        )
        .route(
            "/v1/tablesdb/{database_id}/tables/{table_id}",
            get(get_table).delete(delete_table),
        )
        .route(
            "/v1/tablesdb/{database_id}/tables/{table_id}/columns/{kind}",
            post(create_column).delete(delete_column),
        )
        .route(
            "/v1/tablesdb/{database_id}/tables/{table_id}/rows",
            get(list_rows).post(create_row),
        )
        .route(
            "/v1/tablesdb/{database_id}/tables/{table_id}/rows/{row_id}",
            get(get_row)
                .put(upsert_row)
                .patch(update_row)
                .delete(delete_row),
        )
}

fn table_json(table: &CollectionRow) -> Value {
    let columns = Attribute::from_value(&table.attributes);
    // The schema tooling only needs to know whether the common userId index is
    // present; Openwrite has no separate index registry yet.
    let indexes = if columns.iter().any(|column| column.key == "userId") {
        json!([{
            "key": "userId_idx",
            "type": "key",
            "status": "available",
            "attributes": ["userId"],
            "columns": ["userId"],
        }])
    } else {
        json!([])
    };
    json!({
        "$id": table.id,
        "$createdAt": crate::models::ts(table.created_at),
        "$updatedAt": crate::models::ts(table.updated_at),
        "$permissions": table.permissions,
        "databaseId": table.database_id,
        "name": table.name,
        "enabled": true,
        "rowSecurity": table.document_security,
        "columns": columns.iter().map(attribute_json).collect::<Vec<_>>(),
        "indexes": indexes,
    })
}

fn row_json(document: Value, table_id: &str) -> Value {
    let mut row = document;
    if let Some(object) = row.as_object_mut() {
        object.remove("$collectionId");
        object.insert("$tableId".into(), json!(table_id));
    }
    row
}

fn rows_page(page: Value, table_id: &str) -> Value {
    let total = page.get("total").cloned().unwrap_or_else(|| json!(0));
    let rows = page
        .get("documents")
        .and_then(Value::as_array)
        .map(|documents| {
            documents
                .iter()
                .cloned()
                .map(|document| row_json(document, table_id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({ "total": total, "rows": rows })
}

pub async fn list_tables(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path(database_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let tables =
        store::list_collections(&state.writer().await, &ctx.project.id, &database_id).await?;
    Ok(Json(json!({
        "total": tables.len(),
        "tables": tables.iter().map(table_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateTableBody {
    #[serde(rename = "tableId")]
    pub table_id: Option<String>,
    pub name: String,
    pub permissions: Option<Value>,
    #[serde(rename = "rowSecurity")]
    pub row_security: Option<bool>,
}

pub async fn create_table(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path(database_id): Path<String>,
    Json(body): Json<CreateTableBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ctx.require_key()?;
    let table = store::create_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        body.table_id,
        body.name,
        json!([]),
        body.permissions.as_ref(),
        body.row_security.unwrap_or(true),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(table_json(&table))))
}

pub async fn get_table(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, table_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let table = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &table_id,
    )
    .await?;
    Ok(Json(table_json(&table)))
}

pub async fn delete_table(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, table_id)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    ctx.require_key()?;
    store::delete_collection(&state.writer().await, &ctx.project.id, &database_id, &table_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ColumnBody {
    pub key: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub array: bool,
    pub size: Option<i64>,
    pub default: Option<Value>,
    pub elements: Option<Vec<String>>,
}

pub async fn create_column(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, table_id, kind)): Path<(String, String, String)>,
    Json(body): Json<ColumnBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ctx.require_key()?;
    if !crate::schema::ATTRIBUTE_TYPES.contains(&kind.as_str()) {
        return Err(ApiError::bad_request(format!(
            "Unsupported column type '{kind}'"
        )));
    }
    let payload = json!({
        "key": body.key,
        "type": kind,
        "required": body.required,
        "array": body.array,
        "size": body.size,
        "default": body.default,
        "elements": body.elements,
    });
    let table = store::add_attribute(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &table_id,
        payload,
    )
    .await?;
    let column = Attribute::from_value(&table.attributes)
        .into_iter()
        .find(|column| column.key == body.key)
        .map(|column| attribute_json(&column))
        .unwrap_or_else(|| json!({}));
    Ok((StatusCode::CREATED, Json(column)))
}

pub async fn delete_column(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, table_id, key)): Path<(String, String, String)>,
) -> ApiResult<impl IntoResponse> {    ctx.require_key()?;
    store::delete_attribute(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &table_id,
        &key,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_rows(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, table_id)): Path<(String, String)>,
    RawQuery(raw): RawQuery,
) -> ApiResult<Json<Value>> {
    let table = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &table_id,
    )
    .await?;
    let page = documents::list_documents(
        &state.reader().await,
        DocAccess::from_ctx(&ctx),
        &table,
        &query_pairs(raw),
    )
    .await?;
    Ok(Json(rows_page(page, &table_id)))
}

#[derive(Deserialize)]
pub struct RowBody {
    #[serde(rename = "rowId")]
    pub row_id: Option<String>,
    pub data: Option<Value>,
    pub permissions: Option<Value>,
}

fn row_data(body: RowBody) -> (Value, Option<Value>) {
    (body.data.unwrap_or_else(|| json!({})), body.permissions)
}

pub async fn create_row(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, table_id)): Path<(String, String)>,
    Json(body): Json<RowBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let table = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &table_id,
    )
    .await?;
    let row = documents::create_document(
        &state.writer().await,
        DocAccess::from_ctx(&ctx),
        &table,
        body.row_id,
        body.data.unwrap_or_else(|| json!({})),
        body.permissions.as_ref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(row_json(row, &table_id))))
}

pub async fn get_row(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, table_id, row_id)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let table = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &table_id,
    )
    .await?;
    let row = documents::get_document(
        &state.reader().await,
        DocAccess::from_ctx(&ctx),
        &table,
        &row_id,
    )
    .await?;
    Ok(Json(row_json(row, &table_id)))
}

pub async fn update_row(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, table_id, row_id)): Path<(String, String, String)>,
    Json(body): Json<RowBody>,
) -> ApiResult<Json<Value>> {
    let table = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &table_id,
    )
    .await?;
    let row = documents::update_document(
        &state.writer().await,
        DocAccess::from_ctx(&ctx),
        &table,
        &row_id,
        body.data.unwrap_or_else(|| json!({})),
        body.permissions.as_ref(),
    )
    .await?;
    Ok(Json(row_json(row, &table_id)))
}

pub async fn upsert_row(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, table_id, row_id)): Path<(String, String, String)>,
    Json(body): Json<RowBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let table = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &table_id,
    )
    .await?;
    let access = DocAccess::from_ctx(&ctx);
    let pool = state.writer().await;
    let (data, permissions) = row_data(body);
    match documents::get_document(&pool, access, &table, &row_id).await {
        Ok(_) => {
            let row = documents::update_document(
                &pool,
                access,
                &table,
                &row_id,
                data,
                permissions.as_ref(),
            )
            .await?;
            Ok((StatusCode::OK, Json(row_json(row, &table_id))))
        }
        Err(error) if error.status == StatusCode::NOT_FOUND => {
            let row = documents::create_document(
                &pool,
                access,
                &table,
                Some(row_id),
                data,
                permissions.as_ref(),
            )
            .await?;
            Ok((StatusCode::CREATED, Json(row_json(row, &table_id))))
        }
        Err(error) => Err(error),
    }
}

pub async fn delete_row(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, table_id, row_id)): Path<(String, String, String)>,
) -> ApiResult<impl IntoResponse> {
    let table = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &table_id,
    )
    .await?;
    documents::delete_document(
        &state.writer().await,
        DocAccess::from_ctx(&ctx),
        &table,
        &row_id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

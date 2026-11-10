use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    auth::{ApiCtx, AppState},
    error::ApiResult,
    models::{
        Attribute, CollectionRow, attribute_json, collection_json, database_json,
    },
    store,
};

#[derive(Deserialize)]
pub struct CreateDatabaseBody {
    #[serde(rename = "databaseId")]
    pub database_id: Option<String>,
    pub name: String,
}

pub async fn list_databases(
    State(state): State<AppState>,
    ctx: ApiCtx,
) -> ApiResult<Json<Value>> {
    let rows = store::list_databases(&state.writer().await, &ctx.project.id).await?;
    Ok(Json(json!({
        "total": rows.len(),
        "databases": rows.iter().map(database_json).collect::<Vec<_>>(),
    })))
}

pub async fn create_database(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Json(body): Json<CreateDatabaseBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ctx.require_key()?;
    let row = store::create_database(
        &state.writer().await,
        &ctx.project.id,
        body.database_id,
        body.name,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(database_json(&row))))
}

pub async fn get_database(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path(database_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let row = store::get_database(&state.writer().await, &ctx.project.id, &database_id).await?;
    Ok(Json(database_json(&row)))
}

pub async fn delete_database(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path(database_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    ctx.require_key()?;
    store::delete_database(&state.writer().await, &ctx.project.id, &database_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_collections(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path(database_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let rows =
        store::list_collections(&state.writer().await, &ctx.project.id, &database_id).await?;
    Ok(Json(json!({
        "total": rows.len(),
        "collections": rows.iter().map(collection_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateCollectionBody {
    #[serde(rename = "collectionId")]
    pub collection_id: Option<String>,
    pub name: String,
    pub permissions: Option<Value>,
    #[serde(rename = "documentSecurity")]
    pub document_security: Option<bool>,
}

pub async fn create_collection(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path(database_id): Path<String>,
    Json(body): Json<CreateCollectionBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ctx.require_key()?;
    let row = store::create_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        body.collection_id,
        body.name,
        json!([]),
        body.permissions.as_ref(),
        body.document_security.unwrap_or(true),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(collection_json(&row))))
}

pub async fn get_collection(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, collection_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let row = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &collection_id,
    )
    .await?;
    Ok(Json(collection_json(&row)))
}

pub async fn delete_collection(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, collection_id)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    ctx.require_key()?;
    store::delete_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &collection_id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct AttributeBody {
    pub key: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub array: bool,
    pub size: Option<i64>,
    pub default: Option<Value>,
    pub elements: Option<Vec<String>>,
}

pub async fn create_attribute(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, collection_id, kind)): Path<(String, String, String)>,
    Json(body): Json<AttributeBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ctx.require_key()?;
    if !crate::schema::ATTRIBUTE_TYPES.contains(&kind.as_str()) {
        return Err(crate::error::ApiError::bad_request(format!(
            "Unsupported attribute type '{kind}'"
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
    let collection = store::add_attribute(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &collection_id,
        payload,
    )
    .await?;
    let attrs = Attribute::from_value(&collection.attributes);
    let attr = attrs
        .into_iter()
        .find(|a| a.key == body.key)
        .map(|a| attribute_json(&a))
        .unwrap_or_else(|| json!({}));
    Ok((StatusCode::CREATED, Json(attr)))
}

pub async fn delete_attribute(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, collection_id, key)): Path<(String, String, String)>,
) -> ApiResult<impl IntoResponse> {
    ctx.require_key()?;
    store::delete_attribute(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &collection_id,
        &key,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[allow(dead_code)]
fn _collection(_: CollectionRow) {}

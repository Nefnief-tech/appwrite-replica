use axum::{
    Json,
    extract::{Path, RawQuery, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    auth::{ApiCtx, AppState},
    error::ApiResult,
    documents::{DocAccess, create_document, delete_document, get_document, list_documents, update_document},
    store,
};

fn query_pairs(raw: Option<String>) -> Vec<(String, String)> {
    raw.map(|q| {
        url::form_urlencoded::parse(q.as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    })
    .unwrap_or_default()
}

pub async fn list(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, collection_id)): Path<(String, String)>,
    RawQuery(raw): RawQuery,
) -> ApiResult<Json<Value>> {
    let collection = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &collection_id,
    )
    .await?;
    let access = DocAccess::from_ctx(&ctx);
    let result = list_documents(
        &state.reader().await,
        access,
        &collection,
        &query_pairs(raw),
    )
    .await?;
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct CreateDocumentBody {
    #[serde(rename = "documentId")]
    pub document_id: Option<String>,
    #[serde(default)]
    pub data: Value,
    pub permissions: Option<Value>,
}

pub async fn create(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, collection_id)): Path<(String, String)>,
    Json(body): Json<CreateDocumentBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let collection = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &collection_id,
    )
    .await?;
    let access = DocAccess::from_ctx(&ctx);
    let document = create_document(
        &state.writer().await,
        access,
        &collection,
        body.document_id,
        body.data,
        body.permissions.as_ref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(document)))
}

pub async fn get(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, collection_id, document_id)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let collection = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &collection_id,
    )
    .await?;
    let access = DocAccess::from_ctx(&ctx);
    let document = get_document(
        &state.reader().await,
        access,
        &collection,
        &document_id,
    )
    .await?;
    Ok(Json(document))
}

#[derive(Deserialize)]
pub struct UpdateDocumentBody {
    #[serde(default)]
    pub data: Option<Value>,
    pub permissions: Option<Value>,
}

pub async fn update(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, collection_id, document_id)): Path<(String, String, String)>,
    Json(body): Json<UpdateDocumentBody>,
) -> ApiResult<Json<Value>> {
    let collection = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &collection_id,
    )
    .await?;
    let access = DocAccess::from_ctx(&ctx);
    let document = update_document(
        &state.writer().await,
        access,
        &collection,
        &document_id,
        body.data.unwrap_or_else(|| serde_json::json!({})),
        body.permissions.as_ref(),
    )
    .await?;
    Ok(Json(document))
}

pub async fn delete(
    State(state): State<AppState>,
    ctx: ApiCtx,
    Path((database_id, collection_id, document_id)): Path<(String, String, String)>,
) -> ApiResult<impl IntoResponse> {
    let collection = store::get_collection(
        &state.writer().await,
        &ctx.project.id,
        &database_id,
        &collection_id,
    )
    .await?;
    let access = DocAccess::from_ctx(&ctx);
    delete_document(&state.writer().await, access, &collection, &document_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

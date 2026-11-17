use axum::{
    Json,
    extract::{Path, Query, RawQuery, State},
    http::StatusCode,
    response::IntoResponse,
};
use axum_extra::extract::CookieJar;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    auth::{
        AdminAuth, AppState, admin_cookie, build_cookie, create_admin_session, destroy_admin_session,
        destroy_all_user_sessions, no_store_cookie, rate_limit,
    },
    crypto::{dsn_hint, validate_dsn, verify_password},
    documents::DocAccess,
    error::{ApiError, ApiResult},
    models::{
        AdminRow, Attribute, ProjectRow, UserRow, attribute_json, collection_json, database_json,
        project_json, ts, user_json,
    },
    nodes::{check_single, load_registry, lazy_pool, save_leader, verify_writable},
    store,
    usage,
};

fn admin_json(admin: &AdminRow) -> Value {
    json!({
        "id": admin.id,
        "email": admin.email,
        "name": admin.name,
        "createdAt": ts(admin.created_at),
    })
}

async fn admin_project_json(state: &AppState, project: &ProjectRow) -> ApiResult<Value> {
    let pool = state.writer().await;
    let (users, databases, collections) = store::project_counts(&pool, &project.id).await?;
    Ok(json!({
        "id": project.id,
        "name": project.name,
        "apiKeyPrefix": project.api_key_prefix,
        "createdAt": ts(project.created_at),
        "updatedAt": ts(project.updated_at),
        "counts": {
            "users": users,
            "databases": databases,
            "collections": collections,
        },
    }))
}

fn admin_user_json(user: &UserRow) -> Value {
    let mut value = user_json(user);
    if let Some(obj) = value.as_object_mut() {
        obj.insert("imported".into(), json!(user.imported));
        obj.insert("hasPassword".into(), json!(user.password_hash.is_some()));
    }
    value
}

async fn ensure_project(state: &AppState, project_id: &str) -> ApiResult<ProjectRow> {
    store::get_project(&state.writer().await, project_id).await
}

async fn reload_registry(state: &AppState) -> ApiResult<()> {
    let pool = state.writer().await;
    let registry = load_registry(&pool, state.cipher(), state.cfg().read_routing)
        .await
        .map_err(|err| ApiError::internal(format!("could not reload nodes: {err}")))?;
    *state.nodes().write().await = registry;
    Ok(())
}

#[derive(Deserialize)]
pub struct LoginBody {
    pub email: String,
    pub password: String,
}

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<LoginBody>,
) -> ApiResult<(StatusCode, CookieJar, Json<Value>)> {
    let email = body.email.trim().to_lowercase();
    rate_limit(&state, &format!("admin:{}", crate::crypto::sha256_hex(&email)), 10).await?;
    let admin = sqlx::query_as::<_, AdminRow>(
        "select id, email, name, password_hash, created_at from ow_admins where email = $1",
    )
    .bind(&email)
    .fetch_optional(&state.writer().await)
    .await?
    .ok_or_else(|| ApiError::unauthorized("Invalid email or password"))?;
    let valid = verify_password(body.password, admin.password_hash.clone())
        .await
        .map_err(ApiError::internal)?;
    if !valid {
        return Err(ApiError::unauthorized("Invalid email or password"));
    }
    crate::auth::clear_rate_limit(&state, &format!("admin:{}", crate::crypto::sha256_hex(&email)))
        .await?;
    let view = admin_json(&admin);
    let token = create_admin_session(&state, &view).await?;
    let cookie = build_cookie(state.cfg(), admin_cookie().into(), token, 7);
    Ok((StatusCode::OK, jar.add(cookie), Json(view)))
}

pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<(CookieJar, StatusCode)> {
    if let Some(cookie) = jar.get(admin_cookie()) {
        destroy_admin_session(&state, cookie.value()).await?;
    }
    let cookie = no_store_cookie(state.cfg(), admin_cookie().into());
    Ok((jar.add(cookie), StatusCode::NO_CONTENT))
}

pub async fn me(admin: AdminAuth) -> Json<Value> {
    Json(admin.admin)
}

pub async fn overview(State(state): State<AppState>, _admin: AdminAuth) -> ApiResult<Json<Value>> {
    let pool = state.writer().await;
    let projects = store::list_projects(&pool).await?;
    let mut project_views = Vec::with_capacity(projects.len());
    for project in &projects {
        project_views.push(admin_project_json(&state, project).await?);
    }
    let users: i64 = sqlx::query_scalar("select count(*) from ow_users")
        .fetch_one(&pool)
        .await?;
    let databases: i64 = sqlx::query_scalar("select count(*) from ow_databases")
        .fetch_one(&pool)
        .await?;
    let collections: i64 = sqlx::query_scalar("select count(*) from ow_collections")
        .fetch_one(&pool)
        .await?;
    let documents = document_total(&pool).await?;
    let nodes = state.nodes().read().await.view().await;
    Ok(Json(json!({
        "totals": {
            "projects": projects.len(),
            "users": users,
            "databases": databases,
            "collections": collections,
            "documents": documents,
        },
        "projects": project_views,
        "nodes": nodes,
    })))
}

async fn document_total(pool: &sqlx::PgPool) -> ApiResult<i64> {
    let tables: Vec<(String, String, String)> = sqlx::query_as(
        "select d.project_id, d.id, c.id from ow_collections c join ow_databases d on d.project_id = c.project_id and d.id = c.database_id",
    )
    .fetch_all(pool)
    .await?;
    let mut total = 0i64;
    for (project_id, database_id, collection_id) in tables {
        let path = crate::db::physical_path(&project_id, &database_id, &collection_id);
        let count: i64 = sqlx::query_scalar(&format!("select count(*) from {path}"))
            .fetch_one(pool)
            .await
            .map_err(|err| ApiError::internal(format!("could not count {path}: {err}")))?;
        total += count;
    }
    Ok(total)
}

pub async fn list_nodes(State(state): State<AppState>, _admin: AdminAuth) -> Json<Value> {
    let registry = state.nodes().read().await;
    Json(json!({
        "writerId": registry.writer_id,
        "nodes": registry.view().await,
    }))
}

#[derive(Deserialize)]
pub struct AddNodeBody {
    pub name: String,
    pub dsn: String,
    pub role: Option<String>,
}

pub async fn add_node(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(body): Json<AddNodeBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let name = body.name.trim();
    if name.is_empty() || name.len() > 64 {
        return Err(ApiError::bad_request(
            "Node name must be between 1 and 64 characters",
        ));
    }
    validate_dsn(&body.dsn).map_err(ApiError::bad_request)?;
    let role = body.role.as_deref().unwrap_or("follower");
    if !matches!(role, "leader" | "follower") {
        return Err(ApiError::bad_request("Node role must be leader or follower"));
    }
    let dsn_hint = dsn_hint(&body.dsn);
    let id = insert_node(&state, name, &body.dsn, role).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": id, "name": name, "role": role, "dsnHint": dsn_hint })),
    ))
}

/// Shared by manual registration and Docker provisioning so both paths validate,
/// encrypt and reload the registry identically.
pub(crate) async fn insert_node(
    state: &AppState,
    name: &str,
    dsn: &str,
    role: &str,
) -> ApiResult<Uuid> {
    if role == "leader" {
        let leaders: i64 =
            sqlx::query_scalar("select count(*) from ow_nodes where role = 'leader'")
                .fetch_one(&state.writer().await)
                .await?;
        if leaders > 0 {
            return Err(ApiError::bad_request(
                "A leader already exists. Promote an existing follower instead.",
            ));
        }
        let pool = lazy_pool(dsn, 4).map_err(|err| ApiError::bad_request(err.to_string()))?;
        verify_writable(&pool).await?;
    }
    let id = Uuid::new_v4();
    let encrypted = state.cipher().encrypt(dsn)?;
    sqlx::query("insert into ow_nodes (id, name, dsn, role) values ($1, $2, $3, $4)")
        .bind(id)
        .bind(name)
        .bind(encrypted)
        .bind(role)
        .execute(&state.writer().await)
        .await
        .map_err(|err| {
            if matches!(&err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505")) {
                ApiError::conflict("A node with this name already exists")
            } else {
                err.into()
            }
        })?;
    if role == "leader" {
        save_leader(state.cfg(), state.cipher(), dsn)
            .map_err(|err| ApiError::internal(format!("could not save leader file: {err}")))?;
    }
    reload_registry(state).await?;
    Ok(id)
}

pub async fn remove_node(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(node_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let id = Uuid::parse_str(&node_id)
        .map_err(|_| ApiError::bad_request("Invalid node ID"))?;
    delete_node(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn delete_node(state: &AppState, id: Uuid) -> ApiResult<()> {
    if state.nodes().read().await.writer_id == id {
        return Err(ApiError::bad_request(
            "The active leader cannot be removed. Promote another node first.",
        ));
    }
    let result = sqlx::query("delete from ow_nodes where id = $1")
        .bind(id)
        .execute(&state.writer().await)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("Node not found"));
    }
    reload_registry(state).await?;
    Ok(())
}

pub async fn check_node(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(node_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let id = Uuid::parse_str(&node_id)
        .map_err(|_| ApiError::bad_request("Invalid node ID"))?;
    Ok(Json(check_single(state.nodes(), id).await?))
}

pub async fn promote_node(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(node_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let id = Uuid::parse_str(&node_id)
        .map_err(|_| ApiError::bad_request("Invalid node ID"))?;
    // Read the target from the in-memory registry: the old leader is already fenced
    // when promotion happens, so it cannot be queried for its own replacement.
    let (role, dsn) = {
        let registry = state.nodes().read().await;
        if registry.writer_id == id {
            return Err(ApiError::bad_request("This node is already the leader"));
        }
        let node = registry
            .find(id)
            .ok_or_else(|| ApiError::not_found("Node not found"))?;
        let role = node.role.try_read().map(|r| *r).unwrap_or(crate::nodes::NodeRole::Retired);
        (role, node.dsn.clone())
    };
    if role == crate::nodes::NodeRole::Retired {
        return Err(ApiError::bad_request(
            "This node is retired. Rebuild it as a replica before promoting.",
        ));
    }
    let pool = lazy_pool(&dsn, 8).map_err(|err| ApiError::bad_request(err.to_string()))?;
    verify_writable(&pool).await?;

    let mut tx = pool.begin().await.map_err(|err| {
        ApiError::bad_request(format!("Could not connect to the promotion target: {err}"))
    })?;
    sqlx::query("update ow_nodes set role = 'retired', updated_at = now() where role = 'leader'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("update ow_nodes set role = 'leader', updated_at = now() where id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    save_leader(state.cfg(), state.cipher(), &dsn)
        .map_err(|err| ApiError::internal(format!("could not save leader file: {err}")))?;

    let registry = load_registry(&pool, state.cipher(), state.cfg().read_routing)
        .await
        .map_err(|err| ApiError::internal(format!("could not load promoted node: {err}")))?;
    *state.nodes().write().await = registry;
    tracing::info!("promoted node {id} to leader");
    Ok(Json(json!({ "promoted": id })))
}

pub async fn list_projects(
    State(state): State<AppState>,
    _admin: AdminAuth,
) -> ApiResult<Json<Value>> {
    let projects = store::list_projects(&state.writer().await).await?;
    let mut views = Vec::with_capacity(projects.len());
    for project in &projects {
        views.push(admin_project_json(&state, project).await?);
    }
    Ok(Json(json!({ "projects": views })))
}

#[derive(Deserialize)]
pub struct CreateProjectBody {
    pub id: Option<String>,
    pub name: String,
}

pub async fn create_project(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(body): Json<CreateProjectBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let (project, api_key) = store::create_project(
        &state.writer().await,
        body.id,
        body.name,
    )
    .await?;
    let mut view = admin_project_json(&state, &project).await?;
    if let Some(obj) = view.as_object_mut() {
        obj.insert("apiKey".into(), json!(api_key));
    }
    Ok((StatusCode::CREATED, Json(view)))
}

pub async fn get_project(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(project_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let project = ensure_project(&state, &project_id).await?;
    let databases = store::list_databases(&state.writer().await, &project_id).await?;
    let mut view = admin_project_json(&state, &project).await?;
    if let Some(obj) = view.as_object_mut() {
        obj.insert(
            "databases".into(),
            Value::Array(databases.iter().map(database_json).collect()),
        );
        obj.insert(
            "appwrite".into(),
            json!({
                "endpoint": state.cfg().public_url,
                "projectId": project.id,
                "apiKeyPrefix": project.api_key_prefix,
            }),
        );
    }
    Ok(Json(view))
}

pub async fn delete_project(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(project_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    store::delete_project(&state.writer().await, &project_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn rotate_key(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(project_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (project, api_key) = store::rotate_project_key(&state.writer().await, &project_id).await?;
    Ok(Json(json!({
        "project": project_json(&project),
        "apiKey": api_key,
    })))
}

pub async fn list_users(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(project_id): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult<Json<Value>> {
    ensure_project(&state, &project_id).await?;
    let mut limit = 25i64;
    let mut offset = 0i64;
    let mut search = String::new();
    if let Some(query) = query {
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            match key.as_ref() {
                "limit" => limit = value.parse().unwrap_or(25),
                "offset" => offset = value.parse().unwrap_or(0),
                "search" => search = value.to_string(),
                _ => {}
            }
        }
    }
    let (total, users) =
        store::list_users(&state.writer().await, &project_id, limit, offset, &search).await?;
    Ok(Json(json!({
        "total": total,
        "users": users.iter().map(admin_user_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateUserBody {
    pub id: Option<String>,
    pub email: String,
    pub name: Option<String>,
    pub password: Option<String>,
}

pub async fn create_user(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(project_id): Path<String>,
    Json(body): Json<CreateUserBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ensure_project(&state, &project_id).await?;
    let user = store::create_user(
        &state.writer().await,
        &project_id,
        body.id,
        &body.email,
        body.name.as_deref().unwrap_or(""),
        body.password,
        false,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(admin_user_json(&user))))
}

#[derive(Deserialize)]
pub struct UpdateUserBody {
    pub name: Option<String>,
    pub status: Option<bool>,
    pub password: Option<String>,
}

pub async fn update_user(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, user_id)): Path<(String, String)>,
    Json(body): Json<UpdateUserBody>,
) -> ApiResult<Json<Value>> {
    ensure_project(&state, &project_id).await?;
    let pool = state.writer().await;
    let mut user = store::find_user(&pool, &project_id, &user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("User not found"))?;

    if let Some(name) = body.name {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 128 {
            return Err(ApiError::bad_request("Name must be between 1 and 128 characters"));
        }
        user = sqlx::query_as::<_, UserRow>(
            "update ow_users set name = $3, updated_at = now() where project_id = $1 and id = $2 returning id, project_id, email, name, password_hash, prefs, status, imported, email_verified, registration, password_update, created_at, updated_at",
        )
        .bind(&project_id)
        .bind(&user_id)
        .bind(name)
        .fetch_one(&pool)
        .await?;
    }
    if let Some(status) = body.status {
        user = sqlx::query_as::<_, UserRow>(
            "update ow_users set status = $3, updated_at = now() where project_id = $1 and id = $2 returning id, project_id, email, name, password_hash, prefs, status, imported, email_verified, registration, password_update, created_at, updated_at",
        )
        .bind(&project_id)
        .bind(&user_id)
        .bind(status)
        .fetch_one(&pool)
        .await?;
        if !status {
            destroy_all_user_sessions(&state, &project_id, &user_id).await?;
        }
    }
    if let Some(password) = body.password {
        user = store::set_user_password(&pool, &project_id, &user_id, &password).await?;
        destroy_all_user_sessions(&state, &project_id, &user_id).await?;
    }
    Ok(Json(admin_user_json(&user)))
}

pub async fn delete_user(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, user_id)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    ensure_project(&state, &project_id).await?;
    store::delete_user(&state.writer().await, &project_id, &user_id).await?;
    destroy_all_user_sessions(&state, &project_id, &user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct CreateDatabaseBody {
    pub id: Option<String>,
    pub name: String,
}

pub async fn list_databases(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(project_id): Path<String>,
) -> ApiResult<Json<Value>> {
    ensure_project(&state, &project_id).await?;
    let rows = store::list_databases(&state.writer().await, &project_id).await?;
    Ok(Json(json!({
        "total": rows.len(),
        "databases": rows.iter().map(database_json).collect::<Vec<_>>(),
    })))
}

pub async fn create_database(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(project_id): Path<String>,
    Json(body): Json<CreateDatabaseBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ensure_project(&state, &project_id).await?;
    let row = store::create_database(
        &state.writer().await,
        &project_id,
        body.id,
        body.name,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(database_json(&row))))
}

pub async fn delete_database(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, database_id)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    ensure_project(&state, &project_id).await?;
    store::delete_database(&state.writer().await, &project_id, &database_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_collections(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, database_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    ensure_project(&state, &project_id).await?;
    let rows =
        store::list_collections(&state.writer().await, &project_id, &database_id).await?;
    Ok(Json(json!({
        "total": rows.len(),
        "collections": rows.iter().map(collection_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateCollectionBody {
    pub id: Option<String>,
    pub name: String,
    pub attributes: Option<Value>,
    pub permissions: Option<Value>,
    #[serde(rename = "documentSecurity")]
    pub document_security: Option<bool>,
}

pub async fn create_collection(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, database_id)): Path<(String, String)>,
    Json(body): Json<CreateCollectionBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ensure_project(&state, &project_id).await?;
    let row = store::create_collection(
        &state.writer().await,
        &project_id,
        &database_id,
        body.id,
        body.name,
        body.attributes.unwrap_or_else(|| json!([])),
        body.permissions.as_ref(),
        body.document_security.unwrap_or(true),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(collection_json(&row))))
}

pub async fn get_collection(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, database_id, collection_id)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let collection =
        admin_collection(&state, &project_id, &database_id, &collection_id).await?;
    Ok(Json(collection_json(&collection)))
}

#[derive(Deserialize)]
pub struct UpdateCollectionBody {
    pub name: Option<String>,
    pub permissions: Option<Value>,
    #[serde(rename = "documentSecurity")]
    pub document_security: Option<bool>,
}

pub async fn update_collection(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, database_id, collection_id)): Path<(String, String, String)>,
    Json(body): Json<UpdateCollectionBody>,
) -> ApiResult<Json<Value>> {
    ensure_project(&state, &project_id).await?;
    let pool = state.writer().await;
    let existing =
        store::get_collection(&pool, &project_id, &database_id, &collection_id).await?;
    let name = match body.name {
        Some(name) => {
            let trimmed = name.trim();
            if trimmed.is_empty() || trimmed.chars().count() > 128 {
                return Err(ApiError::bad_request(
                    "Collection name must be between 1 and 128 characters",
                ));
            }
            trimmed.to_string()
        }
        None => existing.name.clone(),
    };
    let permissions = match body.permissions {
        Some(value) => serde_json::to_value(crate::schema::parse_permissions(Some(&value))?)?,
        None => existing.permissions.clone(),
    };
    let document_security = body.document_security.unwrap_or(existing.document_security);
    let row = sqlx::query_as::<_, crate::models::CollectionRow>(
        "update ow_collections set name = $4, permissions = $5, document_security = $6, updated_at = now() where project_id = $3 and database_id = $1 and id = $2 returning id, project_id, database_id, name, attributes, permissions, document_security, created_at, updated_at",
    )
    .bind(&database_id)
    .bind(&collection_id)
    .bind(&project_id)
    .bind(name)
    .bind(permissions)
    .bind(document_security)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Collection not found"))?;
    Ok(Json(collection_json(&row)))
}

pub async fn delete_collection(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, database_id, collection_id)): Path<(String, String, String)>,
) -> ApiResult<impl IntoResponse> {
    ensure_project(&state, &project_id).await?;
    store::delete_collection(&state.writer().await, &project_id, &database_id, &collection_id)
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
    _admin: AdminAuth,
    Path((project_id, database_id, collection_id, kind)): Path<(String, String, String, String)>,
    Json(body): Json<AttributeBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ensure_project(&state, &project_id).await?;
    if !crate::schema::ATTRIBUTE_TYPES.contains(&kind.as_str()) {
        return Err(ApiError::bad_request(format!(
            "Unsupported attribute type '{kind}'"
        )));
    }
    let collection = store::add_attribute(
        &state.writer().await,
        &project_id,
        &database_id,
        &collection_id,
        json!({
            "key": body.key,
            "type": kind,
            "required": body.required,
            "array": body.array,
            "size": body.size,
            "default": body.default,
            "elements": body.elements,
        }),
    )
    .await?;
    let attr = Attribute::from_value(&collection.attributes)
        .into_iter()
        .find(|a| a.key == body.key)
        .map(|a| attribute_json(&a))
        .unwrap_or_else(|| json!({}));
    Ok((StatusCode::CREATED, Json(attr)))
}

pub async fn delete_attribute(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, database_id, collection_id, key)): Path<(String, String, String, String)>,
) -> ApiResult<impl IntoResponse> {
    ensure_project(&state, &project_id).await?;
    store::delete_attribute(
        &state.writer().await,
        &project_id,
        &database_id,
        &collection_id,
        &key,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn query_pairs(raw: Option<String>) -> Vec<(String, String)> {
    raw.map(|q| {
        url::form_urlencoded::parse(q.as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    })
    .unwrap_or_default()
}

async fn admin_collection(
    state: &AppState,
    project_id: &str,
    database_id: &str,
    collection_id: &str,
) -> ApiResult<crate::models::CollectionRow> {
    ensure_project(state, project_id).await?;
    store::get_collection(
        &state.writer().await,
        project_id,
        database_id,
        collection_id,
    )
    .await
}

pub async fn list_documents(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, database_id, collection_id)): Path<(String, String, String)>,
    RawQuery(raw): RawQuery,
) -> ApiResult<Json<Value>> {
    let collection = admin_collection(&state, &project_id, &database_id, &collection_id).await?;
    let documents = crate::documents::list_documents(
        &state.reader().await,
        DocAccess::server(),
        &collection,
        &query_pairs(raw),
    )
    .await?;
    Ok(Json(documents))
}

#[derive(Deserialize)]
pub struct AdminDocumentBody {
    pub id: Option<String>,
    pub data: Value,
    pub permissions: Option<Value>,
}

pub async fn create_document(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, database_id, collection_id)): Path<(String, String, String)>,
    Json(body): Json<AdminDocumentBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let collection = admin_collection(&state, &project_id, &database_id, &collection_id).await?;
    let document = crate::documents::create_document(
        &state.writer().await,
        DocAccess::server(),
        &collection,
        body.id,
        body.data,
        body.permissions.as_ref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(document)))
}

pub async fn get_document(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, database_id, collection_id, document_id)): Path<(
        String,
        String,
        String,
        String,
    )>,
) -> ApiResult<Json<Value>> {
    let collection = admin_collection(&state, &project_id, &database_id, &collection_id).await?;
    let document = crate::documents::get_document(
        &state.reader().await,
        DocAccess::server(),
        &collection,
        &document_id,
    )
    .await?;
    Ok(Json(document))
}

#[derive(Deserialize)]
pub struct UpdateDocumentBody {
    pub data: Option<Value>,
    pub permissions: Option<Value>,
}

pub async fn update_document(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, database_id, collection_id, document_id)): Path<(
        String,
        String,
        String,
        String,
    )>,
    Json(body): Json<UpdateDocumentBody>,
) -> ApiResult<Json<Value>> {
    let collection = admin_collection(&state, &project_id, &database_id, &collection_id).await?;
    let document = crate::documents::update_document(
        &state.writer().await,
        DocAccess::server(),
        &collection,
        &document_id,
        body.data.unwrap_or_else(|| json!({})),
        body.permissions.as_ref(),
    )
    .await?;
    Ok(Json(document))
}

pub async fn delete_document(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((project_id, database_id, collection_id, document_id)): Path<(
        String,
        String,
        String,
        String,
    )>,
) -> ApiResult<impl IntoResponse> {
    let collection = admin_collection(&state, &project_id, &database_id, &collection_id).await?;
    crate::documents::delete_document(
        &state.writer().await,
        DocAccess::server(),
        &collection,
        &document_id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct UsageParams {
    days: Option<u32>,
}

pub async fn usage(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Query(params): Query<UsageParams>,
) -> ApiResult<Json<Value>> {
    usage::aggregate(state, params.days.unwrap_or(7)).await
}

#[allow(dead_code)]
fn _unused(_: Utc) {}

use chrono::Utc;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::{
    crypto::{
        generate_api_key, generate_id, hash_password, sha256_hex, validate_project_id,
        validate_resource_id,
    },
    db::{physical_path, schema_for_database},
    error::{ApiError, ApiResult},
    models::{Attribute, CollectionRow, DatabaseRow, ProjectRow, UserRow},
    schema::{normalize_attributes, parse_permissions},
};

fn unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}

fn duplicate_object(err: &sqlx::Error) -> bool {
    matches!(
        err,
        sqlx::Error::Database(db)
            if matches!(db.code().as_deref(), Some("23505" | "42P06" | "42P07"))
    )
}

fn validate_name(name: &str, label: &str) -> ApiResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 128 {
        return Err(ApiError::bad_request(format!(
            "{label} must be between 1 and 128 characters"
        )));
    }
    Ok(trimmed.to_string())
}

pub fn normalize_email(email: &str) -> ApiResult<String> {
    let email = email.trim().to_lowercase();
    let valid = email.len() <= 254
        && email.split('@').count() == 2
        && email.contains('.')
        && !email.contains(char::is_whitespace);
    if !valid {
        return Err(ApiError::bad_request("A valid email address is required"));
    }
    Ok(email)
}

pub async fn get_project(pool: &PgPool, id: &str) -> ApiResult<ProjectRow> {
    sqlx::query_as::<_, ProjectRow>(
        "select id, name, api_key_hash, api_key_prefix, created_at, updated_at from ow_projects where id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Project not found"))
}

pub async fn list_projects(pool: &PgPool) -> ApiResult<Vec<ProjectRow>> {
    Ok(sqlx::query_as::<_, ProjectRow>(
        "select id, name, api_key_hash, api_key_prefix, created_at, updated_at from ow_projects order by created_at desc",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn create_project(
    pool: &PgPool,
    id: Option<String>,
    name: String,
) -> ApiResult<(ProjectRow, String)> {
    let id = match id.filter(|v| !v.trim().is_empty()) {
        Some(id) => {
            validate_project_id(id.trim()).map_err(ApiError::bad_request)?;
            id.trim().to_string()
        }
        None => generate_id(),
    };
    let name = validate_name(&name, "Project name")?;
    let api_key = generate_api_key();
    let prefix: String = api_key.chars().take(12).collect();
    let hash = sha256_hex(&api_key);
    let row = sqlx::query_as::<_, ProjectRow>(
        "insert into ow_projects (id, name, api_key_hash, api_key_prefix) values ($1, $2, $3, $4) returning id, name, api_key_hash, api_key_prefix, created_at, updated_at",
    )
    .bind(&id)
    .bind(&name)
    .bind(hash)
    .bind(prefix)
    .fetch_one(pool)
    .await
    .map_err(|err| {
        if unique_violation(&err) {
            ApiError::conflict("A project with this ID already exists")
        } else {
            err.into()
        }
    })?;
    Ok((row, api_key))
}

pub async fn rotate_project_key(pool: &PgPool, id: &str) -> ApiResult<(ProjectRow, String)> {
    let api_key = generate_api_key();
    let prefix: String = api_key.chars().take(12).collect();
    let hash = sha256_hex(&api_key);
    let row = sqlx::query_as::<_, ProjectRow>(
        "update ow_projects set api_key_hash = $2, api_key_prefix = $3, updated_at = now() where id = $1 returning id, name, api_key_hash, api_key_prefix, created_at, updated_at",
    )
    .bind(id)
    .bind(hash)
    .bind(prefix)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Project not found"))?;
    Ok((row, api_key))
}

pub async fn project_counts(pool: &PgPool, project_id: &str) -> ApiResult<(i64, i64, i64)> {
    let users: i64 = sqlx::query_scalar("select count(*) from ow_users where project_id = $1")
        .bind(project_id)
        .fetch_one(pool)
        .await?;
    let databases: i64 =
        sqlx::query_scalar("select count(*) from ow_databases where project_id = $1")
            .bind(project_id)
            .fetch_one(pool)
            .await?;
    let collections: i64 = sqlx::query_scalar(
        "select count(*) from ow_collections c where c.project_id = $1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    Ok((users, databases, collections))
}

pub async fn delete_project(pool: &PgPool, id: &str) -> ApiResult<()> {
    let database_ids: Vec<String> =
        sqlx::query_scalar("select id from ow_databases where project_id = $1")
            .bind(id)
            .fetch_all(pool)
            .await?;
    let mut tx = pool.begin().await?;
    for db in &database_ids {
        sqlx::query(&format!(
            "drop schema if exists {} cascade",
            schema_for_database(id, db)
        ))
        .execute(&mut *tx)
        .await?;
    }
    let result = sqlx::query("delete from ow_projects where id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("Project not found"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn create_user(
    pool: &PgPool,
    project_id: &str,
    id: Option<String>,
    email: &str,
    name: &str,
    password: Option<String>,
    imported: bool,
) -> ApiResult<UserRow> {
    let email = normalize_email(email)?;
    let id = match id.filter(|v| !v.trim().is_empty()) {
        Some(id) => {
            validate_resource_id(id.trim(), "User ID").map_err(ApiError::bad_request)?;
            id.trim().to_string()
        }
        None => generate_id(),
    };
    let name = if name.trim().is_empty() {
        email.split('@').next().unwrap_or("User").to_string()
    } else {
        validate_name(name, "User name")?
    };
    let hash = match password {
        Some(password) => {
            if password.len() < 8 {
                return Err(ApiError::bad_request(
                    "Password must be at least 8 characters",
                ));
            }
            Some(hash_password(password).await.map_err(ApiError::internal)?)
        }
        None => None,
    };
    let existing: Option<String> = sqlx::query_scalar(
        "select id from ow_users where project_id = $1 and email = $2",
    )
    .bind(project_id)
    .bind(&email)
    .fetch_optional(pool)
    .await?;
    if existing.is_some() {
        return Err(ApiError::conflict("A user with this email already exists"));
    }
    let row = sqlx::query_as::<_, UserRow>(
        "insert into ow_users (id, project_id, email, name, password_hash, imported, status) values ($1, $2, $3, $4, $5, $6, true) returning id, project_id, email, name, password_hash, prefs, status, imported, email_verified, registration, password_update, created_at, updated_at",
    )
    .bind(&id)
    .bind(project_id)
    .bind(&email)
    .bind(&name)
    .bind(hash)
    .bind(imported)
    .fetch_one(pool)
    .await
    .map_err(|err| {
        if unique_violation(&err) {
            ApiError::conflict("A user with this ID or email already exists")
        } else {
            err.into()
        }
    })?;
    Ok(row)
}

pub async fn list_users(
    pool: &PgPool,
    project_id: &str,
    limit: i64,
    offset: i64,
    search: &str,
) -> ApiResult<(i64, Vec<UserRow>)> {
    let limit = limit.clamp(1, 500);
    let offset = offset.max(0);
    let pattern = if search.trim().is_empty() {
        String::new()
    } else {
        format!("%{}%", search.trim())
    };
    let total: i64 = sqlx::query_scalar(
        "select count(*) from ow_users where project_id = $1 and ($2 = '' or email ilike $2 or name ilike $2)",
    )
    .bind(project_id)
    .bind(&pattern)
    .fetch_one(pool)
    .await?;
    let rows = sqlx::query_as::<_, UserRow>(
        "select id, project_id, email, name, password_hash, prefs, status, imported, email_verified, registration, password_update, created_at, updated_at from ow_users where project_id = $1 and ($2 = '' or email ilike $2 or name ilike $2) order by created_at desc limit $3 offset $4",
    )
    .bind(project_id)
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok((total, rows))
}

pub async fn find_project(pool: &PgPool, id: &str) -> ApiResult<Option<ProjectRow>> {
    Ok(sqlx::query_as::<_, ProjectRow>(
        "select id, name, api_key_hash, api_key_prefix, created_at, updated_at from ow_projects where id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn find_user_by_email(
    pool: &PgPool,
    project_id: &str,
    email: &str,
) -> ApiResult<Option<UserRow>> {
    Ok(sqlx::query_as::<_, UserRow>(
        "select id, project_id, email, name, password_hash, prefs, status, imported, email_verified, registration, password_update, created_at, updated_at from ow_users where project_id = $1 and email = $2",
    )
    .bind(project_id)
    .bind(email.trim().to_lowercase())
    .fetch_optional(pool)
    .await?)
}

pub async fn find_user(
    pool: &PgPool,
    project_id: &str,
    user_id: &str,
) -> ApiResult<Option<UserRow>> {
    Ok(sqlx::query_as::<_, UserRow>(
        "select id, project_id, email, name, password_hash, prefs, status, imported, email_verified, registration, password_update, created_at, updated_at from ow_users where project_id = $1 and id = $2",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?)
}

pub async fn set_user_password(
    pool: &PgPool,
    project_id: &str,
    user_id: &str,
    password: &str,
) -> ApiResult<UserRow> {
    if password.len() < 8 {
        return Err(ApiError::bad_request(
            "Password must be at least 8 characters",
        ));
    }
    let hash = hash_password(password.to_string())
        .await
        .map_err(ApiError::internal)?;
    sqlx::query_as::<_, UserRow>(
        "update ow_users set password_hash = $3, password_update = now(), updated_at = now() where project_id = $1 and id = $2 returning id, project_id, email, name, password_hash, prefs, status, imported, email_verified, registration, password_update, created_at, updated_at",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(hash)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("User not found"))
}

pub async fn update_user_prefs(
    pool: &PgPool,
    project_id: &str,
    user_id: &str,
    prefs: Value,
) -> ApiResult<UserRow> {
    sqlx::query_as::<_, UserRow>(
        "update ow_users set prefs = $3, updated_at = now() where project_id = $1 and id = $2 returning id, project_id, email, name, password_hash, prefs, status, imported, email_verified, registration, password_update, created_at, updated_at",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(prefs)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("User not found"))
}

pub async fn delete_user(pool: &PgPool, project_id: &str, user_id: &str) -> ApiResult<()> {
    let result = sqlx::query("delete from ow_users where project_id = $1 and id = $2")
        .bind(project_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("User not found"));
    }
    Ok(())
}

pub async fn create_database(
    pool: &PgPool,
    project_id: &str,
    id: Option<String>,
    name: String,
) -> ApiResult<DatabaseRow> {
    let id = match id.filter(|v| !v.trim().is_empty()) {
        Some(id) => {
            validate_resource_id(id.trim(), "Database ID").map_err(ApiError::bad_request)?;
            id.trim().to_string()
        }
        None => generate_id(),
    };
    let name = validate_name(&name, "Database name")?;
    let existing: Option<String> =
        sqlx::query_scalar("select id from ow_databases where project_id = $1 and id = $2")
            .bind(project_id)
            .bind(&id)
            .fetch_optional(pool)
            .await?;
    if existing.is_some() {
        return Err(ApiError::conflict("A database with this ID already exists"));
    }
    let schema = schema_for_database(project_id, &id);
    let mut tx = pool.begin().await?;
    sqlx::query(&format!("create schema {schema}"))
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            if duplicate_object(&err) {
                ApiError::conflict("A database with this ID already exists")
            } else {
                err.into()
            }
        })?;
    let row = sqlx::query_as::<_, DatabaseRow>(
        "insert into ow_databases (id, project_id, name) values ($1, $2, $3) returning id, project_id, name, created_at, updated_at",
    )
    .bind(&id)
    .bind(project_id)
    .bind(&name)
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| {
        if unique_violation(&err) {
            ApiError::conflict("A database with this ID already exists")
        } else {
            err.into()
        }
    })?;
    tx.commit().await?;
    Ok(row)
}

pub async fn get_database(
    pool: &PgPool,
    project_id: &str,
    database_id: &str,
) -> ApiResult<DatabaseRow> {
    sqlx::query_as::<_, DatabaseRow>(
        "select id, project_id, name, created_at, updated_at from ow_databases where project_id = $1 and id = $2",
    )
    .bind(project_id)
    .bind(database_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Database not found"))
}

pub async fn list_databases(pool: &PgPool, project_id: &str) -> ApiResult<Vec<DatabaseRow>> {
    Ok(sqlx::query_as::<_, DatabaseRow>(
        "select id, project_id, name, created_at, updated_at from ow_databases where project_id = $1 order by created_at asc",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?)
}

pub async fn delete_database(
    pool: &PgPool,
    project_id: &str,
    database_id: &str,
) -> ApiResult<()> {
    get_database(pool, project_id, database_id).await?;
    let mut tx = pool.begin().await?;
    sqlx::query(&format!(
        "drop schema if exists {} cascade",
        schema_for_database(project_id, database_id)
    ))
    .execute(&mut *tx)
    .await?;
    sqlx::query("delete from ow_databases where project_id = $1 and id = $2")
        .bind(project_id)
        .bind(database_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn create_collection(
    pool: &PgPool,
    project_id: &str,
    database_id: &str,
    id: Option<String>,
    name: String,
    attributes: Value,
    permissions: Option<&Value>,
    document_security: bool,
) -> ApiResult<CollectionRow> {
    get_database(pool, project_id, database_id).await?;
    let id = match id.filter(|v| !v.trim().is_empty()) {
        Some(id) => {
            validate_resource_id(id.trim(), "Collection ID").map_err(ApiError::bad_request)?;
            id.trim().to_string()
        }
        None => generate_id(),
    };
    let name = validate_name(&name, "Collection name")?;
    let attributes = normalize_attributes(attributes)?;
    let permissions = parse_permissions(permissions)?;
    let attrs_json = serde_json::to_value(&attributes)?;
    let perms_json = serde_json::to_value(&permissions)?;
    let path = physical_path(project_id, database_id, &id);

    let mut tx = pool.begin().await?;
    sqlx::query(&format!(
        "create table {path} (
            id text primary key,
            data jsonb not null default '{{}}'::jsonb,
            permissions jsonb not null default '[]'::jsonb,
            created_at timestamptz not null,
            updated_at timestamptz not null
        )"
    ))
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        if duplicate_object(&err) {
            ApiError::conflict("A collection with this ID already exists")
        } else {
            err.into()
        }
    })?;
    sqlx::query(&format!(
        "create index on {path} using gin (permissions)"
    ))
    .execute(&mut *tx)
    .await?;
    sqlx::query(&format!("create index on {path} (created_at)"))
        .execute(&mut *tx)
        .await?;
    let row = sqlx::query_as::<_, CollectionRow>(
        "insert into ow_collections (id, project_id, database_id, name, attributes, permissions, document_security) values ($1, $2, $3, $4, $5, $6, $7) returning id, project_id, database_id, name, attributes, permissions, document_security, created_at, updated_at",
    )
    .bind(&id)
    .bind(project_id)
    .bind(database_id)
    .bind(&name)
    .bind(&attrs_json)
    .bind(&perms_json)
    .bind(document_security)
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| {
        if unique_violation(&err) {
            ApiError::conflict("A collection with this ID already exists")
        } else {
            err.into()
        }
    })?;
    tx.commit().await?;
    Ok(row)
}

pub async fn get_collection(
    pool: &PgPool,
    project_id: &str,
    database_id: &str,
    collection_id: &str,
) -> ApiResult<CollectionRow> {
    sqlx::query_as::<_, CollectionRow>(
        "select c.id, c.project_id, c.database_id, c.name, c.attributes, c.permissions, c.document_security, c.created_at, c.updated_at
         from ow_collections c
         where c.project_id = $1 and c.database_id = $2 and c.id = $3",
    )
    .bind(project_id)
    .bind(database_id)
    .bind(collection_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Collection not found"))
}

pub async fn list_collections(
    pool: &PgPool,
    project_id: &str,
    database_id: &str,
) -> ApiResult<Vec<CollectionRow>> {
    get_database(pool, project_id, database_id).await?;
    Ok(sqlx::query_as::<_, CollectionRow>(
        "select c.id, c.project_id, c.database_id, c.name, c.attributes, c.permissions, c.document_security, c.created_at, c.updated_at
         from ow_collections c
         where c.project_id = $1 and c.database_id = $2
         order by c.created_at asc",
    )
    .bind(project_id)
    .bind(database_id)
    .fetch_all(pool)
    .await?)
}

pub async fn add_attribute(
    pool: &PgPool,
    project_id: &str,
    database_id: &str,
    collection_id: &str,
    attribute: Value,
) -> ApiResult<CollectionRow> {
    let collection = get_collection(pool, project_id, database_id, collection_id).await?;
    let mut attrs = Attribute::from_value(&collection.attributes);
    let normalized = normalize_attributes(json!([attribute]))?;
    let attr = normalized
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::bad_request("Attribute is required"))?;
    if attrs.iter().any(|a| a.key == attr.key) {
        return Err(ApiError::conflict(format!(
            "Attribute '{}' already exists",
            attr.key
        )));
    }
    attrs.push(attr);
    let attrs_json = serde_json::to_value(&attrs)?;
    sqlx::query_as::<_, CollectionRow>(
        "update ow_collections set attributes = $4, updated_at = now() where project_id = $1 and database_id = $2 and id = $3 returning id, project_id, database_id, name, attributes, permissions, document_security, created_at, updated_at",
    )
    .bind(project_id)
    .bind(database_id)
    .bind(collection_id)
    .bind(attrs_json)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

pub async fn delete_attribute(
    pool: &PgPool,
    project_id: &str,
    database_id: &str,
    collection_id: &str,
    key: &str,
) -> ApiResult<CollectionRow> {
    let collection = get_collection(pool, project_id, database_id, collection_id).await?;
    let mut attrs = Attribute::from_value(&collection.attributes);
    let before = attrs.len();
    attrs.retain(|a| a.key != key);
    if attrs.len() == before {
        return Err(ApiError::not_found("Attribute not found"));
    }
    let attrs_json = serde_json::to_value(&attrs)?;
    sqlx::query_as::<_, CollectionRow>(
        "update ow_collections set attributes = $4, updated_at = now() where project_id = $1 and database_id = $2 and id = $3 returning id, project_id, database_id, name, attributes, permissions, document_security, created_at, updated_at",
    )
    .bind(project_id)
    .bind(database_id)
    .bind(collection_id)
    .bind(attrs_json)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

pub async fn delete_collection(
    pool: &PgPool,
    project_id: &str,
    database_id: &str,
    collection_id: &str,
) -> ApiResult<()> {
    get_collection(pool, project_id, database_id, collection_id).await?;
    let mut tx = pool.begin().await?;
    sqlx::query(&format!(
        "drop table if exists {}",
        physical_path(project_id, database_id, collection_id)
    ))
    .execute(&mut *tx)
    .await?;
    sqlx::query("delete from ow_collections where project_id = $1 and database_id = $2 and id = $3")
        .bind(project_id)
        .bind(database_id)
        .bind(collection_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}


#[allow(clippy::too_many_arguments)]
pub async fn import_user(
    pool: &PgPool,
    project_id: &str,
    id: Option<&str>,
    email: &str,
    name: &str,
    hashed_password: Option<String>,
    prefs: &Value,
    email_verified: bool,
    status: bool,
    registration: chrono::DateTime<Utc>,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
) -> ApiResult<bool> {
    let email = normalize_email(email)?;
    let id = match id.filter(|v| validate_resource_id(v, "User ID").is_ok()) {
        Some(id) => id.to_string(),
        None => generate_id(),
    };
    let result = sqlx::query(
        "insert into ow_users (id, project_id, email, name, password_hash, prefs, status, imported, email_verified, registration, password_update, created_at, updated_at)
         values ($1, $2, $3, $4, $5, $6, $7, true, $8, $9, $9, $10, $11)
         on conflict do nothing",
    )
    .bind(&id)
    .bind(project_id)
    .bind(&email)
    .bind(name)
    .bind(hashed_password)
    .bind(prefs)
    .bind(status)
    .bind(email_verified)
    .bind(registration)
    .bind(created_at)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

#[allow(clippy::too_many_arguments)]
pub async fn import_document(
    pool: &PgPool,
    project_id: &str,
    database_id: &str,
    collection_id: &str,
    id: &str,
    data: &Value,
    permissions: &Value,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
) -> ApiResult<bool> {
    let path = physical_path(project_id, database_id, collection_id);
    let result = sqlx::query(&format!(
        "insert into {path} (id, data, permissions, created_at, updated_at) values ($1, $2, $3, $4, $5) on conflict (id) do nothing"
    ))
    .bind(id)
    .bind(data)
    .bind(permissions)
    .bind(created_at)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// server-managed prefs (MFA state) that the public prefs endpoint preserves
pub async fn set_reserved_pref(
    pool: &PgPool,
    project_id: &str,
    user_id: &str,
    key: &str,
    value: Value,
) -> ApiResult<()> {
    let user = find_user(pool, project_id, user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("User not found"))?;
    let mut prefs = user.prefs;
    if let Some(obj) = prefs.as_object_mut() {
        obj.insert(key.to_string(), value);
    }
    update_user_prefs(pool, project_id, user_id, prefs).await?;
    Ok(())
}

/// one-time email/challenge secret; only its hash is stored
pub async fn create_auth_token(
    pool: &PgPool,
    project_id: &str,
    user_id: &str,
    kind: &str,
    secret: &str,
    ttl_seconds: i64,
) -> ApiResult<uuid::Uuid> {
    Ok(sqlx::query_scalar::<_, uuid::Uuid>(
        "insert into ow_auth_tokens (project_id, user_id, kind, secret_hash, expires_at)
         values ($1, $2, $3, $4, now() + ($5 || ' seconds')::interval)
         returning id",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(kind)
    .bind(sha256_hex(secret))
    .bind(ttl_seconds.to_string())
    .fetch_one(pool)
    .await?)
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct AuthToken {
    pub kind: String,
    pub secret_hash: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub used_at: Option<chrono::DateTime<Utc>>,
}

pub async fn get_auth_token(
    pool: &PgPool,
    project_id: &str,
    user_id: &str,
    id: uuid::Uuid,
) -> ApiResult<Option<AuthToken>> {
    Ok(sqlx::query_as::<_, AuthToken>(
        "select kind, secret_hash, expires_at, used_at from ow_auth_tokens
         where id = $1 and project_id = $2 and user_id = $3",
    )
    .bind(id)
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?)
}

pub async fn mark_auth_token_used(pool: &PgPool, id: uuid::Uuid) -> ApiResult<()> {
    sqlx::query("update ow_auth_tokens set used_at = now() where id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn verify_email(
    pool: &PgPool,
    project_id: &str,
    user_id: &str,
) -> ApiResult<UserRow> {
    sqlx::query_as::<_, UserRow>(
        "update ow_users set email_verified = true, updated_at = now()
         where project_id = $1 and id = $2 returning id, project_id, email, name, password_hash, prefs, status, imported, email_verified, registration, password_update, created_at, updated_at",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("User not found"))
}

/// atomically consume an unexpired secret
pub async fn consume_auth_token(
    pool: &PgPool,
    project_id: &str,
    user_id: &str,
    kind: &str,
    secret: &str,
) -> ApiResult<bool> {
    let row = sqlx::query_scalar::<_, uuid::Uuid>(
        "update ow_auth_tokens set used_at = now()
         where id = (
           select id from ow_auth_tokens
           where project_id = $1 and user_id = $2 and kind = $3 and secret_hash = $4
             and used_at is null and expires_at > now()
           order by created_at desc limit 1
           for update skip locked
         )
         returning id",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(kind)
    .bind(sha256_hex(secret))
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct MailTemplate {
    pub kind: String,
    pub subject: String,
    pub body: String,
    pub html_body: String,
    pub updated_at: chrono::DateTime<Utc>,
}

pub async fn get_mail_template(pool: &PgPool, kind: &str) -> ApiResult<Option<MailTemplate>> {
    Ok(sqlx::query_as::<_, MailTemplate>(
        "select kind, subject, body, html_body, updated_at from ow_mail_templates where kind = $1",
    )
    .bind(kind)
    .fetch_optional(pool)
    .await?)
}

pub async fn list_mail_templates(pool: &PgPool) -> ApiResult<Vec<MailTemplate>> {
    Ok(sqlx::query_as::<_, MailTemplate>(
        "select kind, subject, body, html_body, updated_at from ow_mail_templates order by kind",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn set_mail_template(
    pool: &PgPool,
    kind: &str,
    subject: &str,
    body: &str,
    html_body: &str,
) -> ApiResult<()> {
    sqlx::query(
        "insert into ow_mail_templates (kind, subject, body, html_body) values ($1, $2, $3, $4)
         on conflict (kind) do update set subject = excluded.subject, body = excluded.body,
           html_body = excluded.html_body, updated_at = now()",
    )
    .bind(kind)
    .bind(subject)
    .bind(body)
    .bind(html_body)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize_email;

    #[test]
    fn email_is_normalized() {
        assert_eq!(normalize_email(" A@B.COM ").unwrap(), "a@b.com");
        assert!(normalize_email("nope").is_err());
    }
}

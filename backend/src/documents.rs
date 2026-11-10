use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{
    FromRow, PgPool, Postgres, Row,
    postgres::{PgArguments, PgRow},
    query::{Query, QueryAs},
};

use crate::{
    db::physical_path,
    error::{ApiError, ApiResult},
    models::{Attribute, CollectionRow, document_json},
    schema::{
        parse_permissions, permission_allows, prepare_create, prepare_update,
        query_error, scalar_to_string,
    },
};

#[derive(Clone, Copy, Debug)]
pub struct DocAccess<'a> {
    pub is_api_key: bool,
    pub user_id: Option<&'a str>,
}

impl<'a> DocAccess<'a> {
    pub fn server() -> Self {
        Self {
            is_api_key: true,
            user_id: None,
        }
    }

    pub fn from_ctx(ctx: &'a crate::auth::ApiCtx) -> Self {
        Self {
            is_api_key: ctx.is_api_key,
            user_id: ctx.viewer_id(),
        }
    }
}

#[derive(Clone, Debug)]
enum BindValue {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Timestamp(DateTime<Utc>),
    TextArray(Vec<String>),
}

fn bind_all<'q>(sql: &'q str, binds: &[BindValue]) -> Query<'q, Postgres, PgArguments> {
    let mut query = sqlx::query(sql);
    for bind in binds {
        query = match bind {
            BindValue::Text(v) => query.bind(v.clone()),
            BindValue::Int(v) => query.bind(*v),
            BindValue::Float(v) => query.bind(*v),
            BindValue::Bool(v) => query.bind(*v),
            BindValue::Timestamp(v) => query.bind(*v),
            BindValue::TextArray(v) => query.bind(v.clone()),
        };
    }
    query
}

fn bind_all_as<'q, T>(sql: &'q str, binds: &[BindValue]) -> QueryAs<'q, Postgres, T, PgArguments>
where
    T: for<'r> FromRow<'r, PgRow> + Send + Unpin,
{
    let mut query = sqlx::query_as::<_, T>(sql);
    for bind in binds {
        query = match bind {
            BindValue::Text(v) => query.bind(v.clone()),
            BindValue::Int(v) => query.bind(*v),
            BindValue::Float(v) => query.bind(*v),
            BindValue::Bool(v) => query.bind(*v),
            BindValue::Timestamp(v) => query.bind(*v),
            BindValue::TextArray(v) => query.bind(v.clone()),
        };
    }
    query
}

#[derive(Clone, Debug)]
enum Field {
    Id,
    CreatedAt,
    UpdatedAt,
    Attr(Attribute),
}

fn resolve_field(collection: &CollectionRow, name: &str) -> ApiResult<Field> {
    match name {
        "$id" => Ok(Field::Id),
        "$createdAt" => Ok(Field::CreatedAt),
        "$updatedAt" => Ok(Field::UpdatedAt),
        _ => {
            let attrs = Attribute::from_value(&collection.attributes);
            let attr = attrs
                .into_iter()
                .find(|a| a.key == name)
                .ok_or_else(|| query_error(format!("Unknown attribute '{name}' in query")))?;
            Ok(Field::Attr(attr))
        }
    }
}

fn field_sql(field: &Field) -> String {
    match field {
        Field::Id => "id".into(),
        Field::CreatedAt => "created_at".into(),
        Field::UpdatedAt => "updated_at".into(),
        Field::Attr(attr) => {
            let key = attr.key.replace('\'', "''");
            match attr.kind.as_str() {
                "integer" | "float" => format!("(data->>'{key}')::double precision"),
                "boolean" => format!("(data->>'{key}')::boolean"),
                "datetime" => format!("(data->>'{key}')::timestamptz"),
                _ => format!("(data->>'{key}')"),
            }
        }
    }
}

fn order_sql(field: &Field, descending: bool) -> String {
    let direction = if descending { "DESC" } else { "ASC" };
    format!("{} {} NULLS LAST", field_sql(field), direction)
}

fn field_bind(field: &Field, value: &Value) -> ApiResult<BindValue> {
    let text = scalar_to_string(value);
    match field {
        Field::Id => Ok(BindValue::Text(text)),
        Field::CreatedAt | Field::UpdatedAt => {
            let parsed = value
                .as_str()
                .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
                .map(|v| v.with_timezone(&Utc))
                .ok_or_else(|| query_error("Timestamp queries must use an RFC 3339 string"))?;
            Ok(BindValue::Timestamp(parsed))
        }
        Field::Attr(attr) => match attr.kind.as_str() {
            "integer" | "float" => value
                .as_f64()
                .or_else(|| text.parse().ok())
                .map(BindValue::Float)
                .ok_or_else(|| query_error(format!("'{}' expects a number", attr.key))),
            "boolean" => value
                .as_bool()
                .or_else(|| match text.as_str() {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                })
                .map(BindValue::Bool)
                .ok_or_else(|| query_error(format!("'{}' expects a boolean", attr.key))),
            "datetime" => value
                .as_str()
                .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
                .map(|v| BindValue::Timestamp(v.with_timezone(&Utc)))
                .ok_or_else(|| query_error(format!("'{}' expects an RFC 3339 datetime", attr.key))),
            _ => Ok(BindValue::Text(text)),
        },
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FilterOp {
    Equal,
    NotEqual,
    GreaterThan,
    GreaterThanEqual,
    LessThan,
    LessThanEqual,
    StartsWith,
    EndsWith,
    Search,
    IsNull,
    IsNotNull,
}

#[derive(Clone, Debug)]
struct Filter {
    field: Field,
    op: FilterOp,
    value: Option<Value>,
}

#[derive(Clone, Debug)]
struct Order {
    field: Field,
    descending: bool,
}

fn split_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escape = false;
    let mut depth = 0i32;
    for ch in input.chars() {
        if in_string {
            current.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                current.push(ch);
            }
            '(' | '[' | '{' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                args.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        args.push(current.trim().to_string());
    }
    args
}

fn parse_arg(arg: &str) -> Value {
    serde_json::from_str(arg).unwrap_or_else(|_| Value::String(arg.trim_matches('"').to_string()))
}

fn parse_queries(
    collection: &CollectionRow,
    raw_queries: &[(String, String)],
) -> ApiResult<(Vec<Filter>, Vec<Order>, i64, i64)> {
    let mut filters = Vec::new();
    let mut orders = Vec::new();
    let mut limit = 25i64;
    let mut offset = 0i64;

    for (key, value) in raw_queries {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.as_str() {
            "queries" | "queries[]" => {}
            "limit" => {
                limit = value
                    .parse()
                    .map_err(|_| query_error("limit must be a number"))?;
                continue;
            }
            "offset" => {
                offset = value
                    .parse()
                    .map_err(|_| query_error("offset must be a number"))?;
                continue;
            }
            _ => continue,
        }

        let Some(open) = value.find('(') else {
            return Err(query_error(format!("Invalid query '{value}'")));
        };
        if !value.ends_with(')') {
            return Err(query_error(format!("Invalid query '{value}'")));
        }
        let name = &value[..open];
        let args = split_args(&value[open + 1..value.len() - 1]);
        let arg = |idx: usize| -> ApiResult<&str> {
            args.get(idx)
                .map(String::as_str)
                .ok_or_else(|| query_error(format!("Query '{name}' is missing arguments")))
        };

        match name {
            "limit" => {
                let parsed = parse_arg(arg(0)?);
                limit = parsed
                    .as_i64()
                    .or_else(|| parsed.as_str().and_then(|s| s.parse().ok()))
                    .ok_or_else(|| query_error("limit expects a number"))?;
            }
            "offset" => {
                let parsed = parse_arg(arg(0)?);
                offset = parsed
                    .as_i64()
                    .or_else(|| parsed.as_str().and_then(|s| s.parse().ok()))
                    .ok_or_else(|| query_error("offset expects a number"))?;
            }
            "select" => {
                // v1 returns full documents. Selecting fields is accepted for compatibility.
            }
            "orderAsc" | "orderDesc" => {
                let field = parse_arg(arg(0)?);
                let field = field
                    .as_str()
                    .ok_or_else(|| query_error("orderAsc/orderDesc expect an attribute name"))?;
                orders.push(Order {
                    field: resolve_field(collection, field)?,
                    descending: name == "orderDesc",
                });
            }
            "equal" | "notEqual" | "greaterThan" | "greaterThanEqual" | "lessThan"
            | "lessThanEqual" | "startsWith" | "endsWith" | "search" => {
                let field = parse_arg(arg(0)?);
                let field = field
                    .as_str()
                    .ok_or_else(|| query_error(format!("{name} expects an attribute name")))?;
                let field = resolve_field(collection, field)?;
                let value = parse_arg(arg(1)?);
                let op = match name {
                    "equal" => FilterOp::Equal,
                    "notEqual" => FilterOp::NotEqual,
                    "greaterThan" => FilterOp::GreaterThan,
                    "greaterThanEqual" => FilterOp::GreaterThanEqual,
                    "lessThan" => FilterOp::LessThan,
                    "lessThanEqual" => FilterOp::LessThanEqual,
                    "startsWith" => FilterOp::StartsWith,
                    "endsWith" => FilterOp::EndsWith,
                    _ => FilterOp::Search,
                };
                filters.push(Filter {
                    field,
                    op,
                    value: Some(value),
                });
            }
            "isNull" | "isNotNull" => {
                let field = parse_arg(arg(0)?);
                let field = field
                    .as_str()
                    .ok_or_else(|| query_error(format!("{name} expects an attribute name")))?;
                filters.push(Filter {
                    field: resolve_field(collection, field)?,
                    op: if name == "isNull" {
                        FilterOp::IsNull
                    } else {
                        FilterOp::IsNotNull
                    },
                    value: None,
                });
            }
            other => {
                return Err(query_error(format!(
                    "Query '{other}()' is not supported by Openwrite yet"
                )));
            }
        }
    }

    Ok((filters, orders, limit, offset))
}

fn doc_allowed(
    collection: &CollectionRow,
    permissions: &Value,
    action: &str,
    access: DocAccess<'_>,
) -> bool {
    if access.is_api_key {
        return true;
    }
    let collection_permissions: Vec<String> =
        serde_json::from_value(collection.permissions.clone()).unwrap_or_default();
    if !collection.document_security {
        return permission_allows(&collection_permissions, action, access.user_id);
    }
    let doc_permissions: Vec<String> =
        serde_json::from_value(permissions.clone()).unwrap_or_default();
    permission_allows(&doc_permissions, action, access.user_id)
}

fn build_sql(
    collection: &CollectionRow,
    access: DocAccess<'_>,
    raw_queries: &[(String, String)],
) -> ApiResult<(String, Vec<BindValue>, i64, i64, String)> {
    let (filters, orders, limit, offset) = parse_queries(collection, raw_queries)?;
    if limit < 0 || offset < 0 {
        return Err(query_error("limit and offset must be positive"));
    }
    let limit = limit.min(5000);
    let mut where_parts: Vec<String> = Vec::new();
    let mut binds: Vec<BindValue> = Vec::new();

    if !access.is_api_key {
        if collection.document_security {
            let mut entries = vec!["read(\"any\")".to_string()];
            match access.user_id {
                Some(id) => {
                    entries.push("read(\"users\")".to_string());
                    entries.push(format!("read(\"user:{id}\")"));
                }
                None => entries.push("read(\"guests\")".to_string()),
            }
            bind_permission_filter(&mut where_parts, &mut binds, &entries);
        } else {
            let collection_permissions: Vec<String> =
                serde_json::from_value(collection.permissions.clone()).unwrap_or_default();
            if !permission_allows(&collection_permissions, "read", access.user_id) {
                return Err(ApiError::forbidden(
                    "You do not have permission to read documents in this collection",
                ));
            }
        }
    }

    for filter in &filters {
        match filter.op {
            FilterOp::IsNull => {
                where_parts.push(format!("{} IS NULL", field_sql(&filter.field)));
            }
            FilterOp::IsNotNull => {
                where_parts.push(format!("{} IS NOT NULL", field_sql(&filter.field)));
            }
            _ => {
                let value = filter
                    .value
                    .as_ref()
                    .ok_or_else(|| query_error("Filter is missing a value"))?;
                let field_sql = field_sql(&filter.field);
                match filter.op {
                    FilterOp::StartsWith | FilterOp::EndsWith | FilterOp::Search => {
                        let text = scalar_to_string(value);
                        let pattern = match filter.op {
                            FilterOp::StartsWith => format!("{}%", escape_like(&text)),
                            FilterOp::EndsWith => format!("%{}", escape_like(&text)),
                            _ => format!("%{}%", escape_like(&text)),
                        };
                        where_parts.push(format!("{field_sql} ILIKE ${} ESCAPE '\\'", binds.len() + 1));
                        binds.push(BindValue::Text(pattern));
                    }
                    _ => {
                        let op = match filter.op {
                            FilterOp::Equal => "=",
                            FilterOp::NotEqual => "<>",
                            FilterOp::GreaterThan => ">",
                            FilterOp::GreaterThanEqual => ">=",
                            FilterOp::LessThan => "<",
                            FilterOp::LessThanEqual => "<=",
                            _ => unreachable!(),
                        };
                        let bind = field_bind(&filter.field, value)?;
                        where_parts.push(format!("{field_sql} {op} ${}", binds.len() + 1));
                        binds.push(bind);
                    }
                }
            }
        }
    }

    let mut sql = format!(
        "select id, data, permissions, created_at, updated_at from {}",
        physical_path(&collection.project_id, &collection.database_id, &collection.id)
    );
    if !where_parts.is_empty() {
        sql.push_str(" where ");
        sql.push_str(&where_parts.join(" and "));
    }
    if !orders.is_empty() {
        sql.push_str(" order by ");
        sql.push_str(
            &orders
                .iter()
                .map(|o| order_sql(&o.field, o.descending))
                .collect::<Vec<_>>()
                .join(", "),
        );
    } else {
        sql.push_str(" order by created_at asc");
    }

    let count_sql = {
        let mut base = format!(
            "select count(*) from {}",
            physical_path(&collection.project_id, &collection.database_id, &collection.id)
        );
        if !where_parts.is_empty() {
            base.push_str(" where ");
            base.push_str(&where_parts.join(" and "));
        }
        base
    };

    Ok((sql, binds, limit, offset, count_sql))
}

fn bind_permission_filter(where_parts: &mut Vec<String>, binds: &mut Vec<BindValue>, entries: &[String]) {
    if entries.is_empty() {
        where_parts.push("false".into());
    } else {
        where_parts.push(format!("permissions ?| ${}", binds.len() + 1));
        binds.push(BindValue::TextArray(entries.to_vec()));
    }
}

pub async fn list_documents(
    pool: &PgPool,
    access: DocAccess<'_>,
    collection: &CollectionRow,
    raw_queries: &[(String, String)],
) -> ApiResult<Value> {
    let (mut sql, mut binds, limit, offset, count_sql) =
        build_sql(collection, access, raw_queries)?;

    let total: i64 = bind_all(&count_sql, &binds)
        .fetch_one(pool)
        .await?
        .try_get(0)?;

    sql.push_str(&format!(" limit ${} offset ${}", binds.len() + 1, binds.len() + 2));
    binds.push(BindValue::Int(limit));
    binds.push(BindValue::Int(offset));
    let rows: Vec<DocRow> = bind_all_as::<DocRow>(&sql, &binds)
        .fetch_all(pool)
        .await?;
    let documents: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            document_json(
                &row.id,
                &collection.database_id,
                &collection.id,
                &row.data,
                &row.permissions,
                row.created_at,
                row.updated_at,
            )
        })
        .collect();
    Ok(json!({ "total": total, "documents": documents }))
}

#[derive(Debug, sqlx::FromRow)]
struct DocRow {
    id: String,
    data: Value,
    permissions: Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

pub async fn get_document(
    pool: &PgPool,
    access: DocAccess<'_>,
    collection: &CollectionRow,
    document_id: &str,
) -> ApiResult<Value> {
    let row: Option<DocRow> = sqlx::query_as(&format!(
        "select id, data, permissions, created_at, updated_at from {} where id = $1",
        physical_path(&collection.project_id, &collection.database_id, &collection.id)
    ))
    .bind(document_id)
    .fetch_optional(pool)
    .await?;
    let row = row.ok_or_else(|| ApiError::not_found("Document not found"))?;
    if !doc_allowed(collection, &row.permissions, "read", access) {
        return Err(ApiError::forbidden("You do not have permission to read this document"));
    }
    Ok(document_json(
        &row.id,
        &collection.database_id,
        &collection.id,
        &row.data,
        &row.permissions,
        row.created_at,
        row.updated_at,
    ))
}

#[allow(clippy::too_many_arguments)]
pub async fn create_document(
    pool: &PgPool,
    access: DocAccess<'_>,
    collection: &CollectionRow,
    document_id: Option<String>,
    data: Value,
    permissions: Option<&Value>,
) -> ApiResult<Value> {
    if !access.is_api_key {
        let collection_permissions: Vec<String> =
            serde_json::from_value(collection.permissions.clone()).unwrap_or_default();
        if !permission_allows(&collection_permissions, "create", access.user_id) {
            return Err(ApiError::forbidden(
                "You do not have permission to create documents in this collection",
            ));
        }
    }
    let attrs = Attribute::from_value(&collection.attributes);
    let data = prepare_create(&attrs, data)?;
    let document_id = match document_id.filter(|v| !v.trim().is_empty()) {
        Some(id) => {
            crate::crypto::validate_resource_id(id.trim(), "Document ID")
                .map_err(ApiError::bad_request)?;
            id.trim().to_string()
        }
        None => crate::crypto::generate_id(),
    };
    let permissions = match permissions {
        Some(value) => parse_permissions(Some(value))?,
        None => crate::schema::doc_default_permissions(access.user_id),
    };
    let permissions = serde_json::to_value(permissions)?;
    let row: DocRow = sqlx::query_as(&format!(
        "insert into {} (id, data, permissions, created_at, updated_at) values ($1, $2, $3, now(), now()) returning id, data, permissions, created_at, updated_at",
        physical_path(&collection.project_id, &collection.database_id, &collection.id)
    ))
    .bind(&document_id)
    .bind(&data)
    .bind(&permissions)
    .fetch_one(pool)
    .await
    .map_err(|err| {
        if matches!(&err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505")) {
            ApiError::conflict("A document with this ID already exists")
        } else {
            err.into()
        }
    })?;
    Ok(document_json(
        &row.id,
        &collection.database_id,
        &collection.id,
        &row.data,
        &row.permissions,
        row.created_at,
        row.updated_at,
    ))
}

pub async fn update_document(
    pool: &PgPool,
    access: DocAccess<'_>,
    collection: &CollectionRow,
    document_id: &str,
    data: Value,
    permissions: Option<&Value>,
) -> ApiResult<Value> {
    let existing: Option<DocRow> = sqlx::query_as(&format!(
        "select id, data, permissions, created_at, updated_at from {} where id = $1",
        physical_path(&collection.project_id, &collection.database_id, &collection.id)
    ))
    .bind(document_id)
    .fetch_optional(pool)
    .await?;
    let existing = existing.ok_or_else(|| ApiError::not_found("Document not found"))?;
    if !doc_allowed(collection, &existing.permissions, "update", access) {
        return Err(ApiError::forbidden("You do not have permission to update this document"));
    }
    let attrs = Attribute::from_value(&collection.attributes);
    let patch = prepare_update(&attrs, data)?;
    let new_permissions = match permissions {
        Some(value) => Some(serde_json::to_value(parse_permissions(Some(value))?)?),
        None => None,
    };
    let row: DocRow = sqlx::query_as(&format!(
        "update {} set data = data || $2, permissions = coalesce($3, permissions), updated_at = now() where id = $1 returning id, data, permissions, created_at, updated_at",
        physical_path(&collection.project_id, &collection.database_id, &collection.id)
    ))
    .bind(document_id)
    .bind(&patch)
    .bind(&new_permissions)
    .fetch_one(pool)
    .await?;
    Ok(document_json(
        &row.id,
        &collection.database_id,
        &collection.id,
        &row.data,
        &row.permissions,
        row.created_at,
        row.updated_at,
    ))
}

pub async fn delete_document(
    pool: &PgPool,
    access: DocAccess<'_>,
    collection: &CollectionRow,
    document_id: &str,
) -> ApiResult<()> {
    let existing: Option<DocRow> = sqlx::query_as(&format!(
        "select id, data, permissions, created_at, updated_at from {} where id = $1",
        physical_path(&collection.project_id, &collection.database_id, &collection.id)
    ))
    .bind(document_id)
    .fetch_optional(pool)
    .await?;
    let existing = existing.ok_or_else(|| ApiError::not_found("Document not found"))?;
    if !doc_allowed(collection, &existing.permissions, "delete", access) {
        return Err(ApiError::forbidden("You do not have permission to delete this document"));
    }
    sqlx::query(&format!(
        "delete from {} where id = $1",
        physical_path(&collection.project_id, &collection.database_id, &collection.id)
    ))
    .bind(document_id)
    .execute(pool)
    .await?;
    Ok(())
}


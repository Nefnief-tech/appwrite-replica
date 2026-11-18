use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

pub fn ts(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct AdminRow {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct ProjectRow {
    pub id: String,
    pub name: String,
    pub api_key_hash: String,
    pub api_key_prefix: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct UserRow {
    pub id: String,
    #[allow(dead_code)]
    pub project_id: String,
    pub email: String,
    pub name: String,
    pub password_hash: Option<String>,
    pub prefs: Value,
    pub status: bool,
    pub imported: bool,
    pub email_verified: bool,
    pub registration: DateTime<Utc>,
    pub password_update: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct DatabaseRow {
    pub id: String,
    #[allow(dead_code)]
    pub project_id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct CollectionRow {
    pub id: String,
    pub project_id: String,
    pub database_id: String,
    pub name: String,
    pub attributes: Value,
    pub permissions: Value,
    pub document_security: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribute {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub array: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elements: Option<Vec<String>>,
}

impl Attribute {
    pub fn from_value(value: &Value) -> Vec<Self> {
        serde_json::from_value(value.clone()).unwrap_or_default()
    }
}

pub fn attribute_json(attr: &Attribute) -> Value {
    let mut obj = Map::new();
    obj.insert("key".into(), json!(attr.key));
    obj.insert("type".into(), json!(attr.kind));
    obj.insert("status".into(), json!("available"));
    obj.insert("error".into(), json!(""));
    obj.insert("required".into(), json!(attr.required));
    obj.insert("array".into(), json!(attr.array));
    if let Some(size) = attr.size {
        obj.insert("size".into(), json!(size));
    }
    if let Some(default) = &attr.default {
        obj.insert("default".into(), default.clone());
    }
    if let Some(elements) = &attr.elements {
        obj.insert("elements".into(), json!(elements));
    }
    Value::Object(obj)
}

/// MFA state lives under reserved prefs keys that the public prefs endpoint
/// strips, so a client cannot switch its own second factor off.
pub fn mfa_enabled(user: &UserRow) -> bool {
    user.prefs.get("__mfa").and_then(Value::as_bool).unwrap_or(false)
}

pub fn mfa_recovery_codes(user: &UserRow) -> Vec<String> {
    user.prefs
        .get("__mfaRecovery")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

pub fn user_json(user: &UserRow) -> Value {
    json!({
        "$id": user.id,
        "$createdAt": ts(user.created_at),
        "$updatedAt": ts(user.updated_at),
        "name": user.name,
        "registration": ts(user.registration),
        "status": user.status,
        "labels": [],
        "passwordUpdate": ts(user.password_update),
        "email": user.email,
        "phone": "",
        "emailVerification": user.email_verified,
        "phoneVerification": false,
        "mfa": mfa_enabled(user),
        "prefs": user.prefs,
        "targets": [],
    })
}

pub fn project_json(project: &ProjectRow) -> Value {
    json!({
        "$id": project.id,
        "name": project.name,
        "apiKeyPrefix": project.api_key_prefix,
        "$createdAt": ts(project.created_at),
        "$updatedAt": ts(project.updated_at),
    })
}

pub fn database_json(database: &DatabaseRow) -> Value {
    json!({
        "$id": database.id,
        "name": database.name,
        "$createdAt": ts(database.created_at),
        "$updatedAt": ts(database.updated_at),
        "enabled": true,
        "type": "legacy",
    })
}

pub fn collection_json(collection: &CollectionRow) -> Value {
    let attrs = Attribute::from_value(&collection.attributes);
    let attributes: Vec<Value> = attrs.iter().map(attribute_json).collect();
    json!({
        "$id": collection.id,
        "$createdAt": ts(collection.created_at),
        "$updatedAt": ts(collection.updated_at),
        "$permissions": collection.permissions,
        "databaseId": collection.database_id,
        "name": collection.name,
        "enabled": true,
        "documentSecurity": collection.document_security,
        "attributes": attributes,
        "indexes": [],
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserSession {
    pub user_id: String,
    pub session_id: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
    pub expire_at: DateTime<Utc>,
    #[serde(default)]
    pub mfa_pending: bool,
}

pub fn session_json(session: &UserSession, current: bool) -> Value {
    json!({
        "$id": session.session_id,
        "$createdAt": ts(session.created_at),
        "$updatedAt": ts(session.created_at),
        "userId": session.user_id,
        "expire": ts(session.expire_at),
        "provider": "email",
        "providerUid": session.email,
        "providerAccessToken": "",
        "providerAccessTokenExpiry": "",
        "providerRefreshToken": "",
        "ip": "",
        "osCode": "",
        "osName": "",
        "osVersion": "",
        "clientType": "",
        "clientName": "",
        "clientVersion": "",
        "clientEngine": "",
        "clientEngineVersion": "",
        "deviceName": "",
        "deviceBrand": "",
        "deviceModel": "",
        "countryCode": "",
        "countryName": "",
        "current": current,
        "factors": [],
        "secret": "",
        "mfaUpdatedAt": "",
    })
}

pub fn document_json(
    id: &str,
    database_id: &str,
    collection_id: &str,
    data: &Value,
    permissions: &Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> Value {
    let mut obj = data.as_object().cloned().unwrap_or_default();
    obj.insert("$id".into(), json!(id));
    obj.insert("$sequence".into(), json!("0"));
    obj.insert("$databaseId".into(), json!(database_id));
    obj.insert("$collectionId".into(), json!(collection_id));
    obj.insert("$createdAt".into(), json!(ts(created_at)));
    obj.insert("$updatedAt".into(), json!(ts(updated_at)));
    obj.insert("$permissions".into(), permissions.clone());
    Value::Object(obj)
}

use std::collections::HashSet;

use chrono::DateTime;
use serde_json::{Map, Value};
#[cfg(test)]
use serde_json::json;

use crate::{
    error::{ApiError, ApiResult},
    models::Attribute,
};

pub const ATTRIBUTE_TYPES: &[&str] = &[
    "string", "integer", "float", "boolean", "datetime", "email", "url", "ip", "enum",
];

pub fn normalize_attributes(value: Value) -> ApiResult<Vec<Attribute>> {
    let items: Vec<Attribute> = serde_json::from_value(value)
        .map_err(|err| ApiError::bad_request(format!("Invalid attributes: {err}")))?;
    let mut attrs = Vec::with_capacity(items.len());
    let mut seen = HashSet::new();
    for mut attr in items {
        crate::crypto::validate_attribute_key(&attr.key).map_err(ApiError::bad_request)?;
        if !seen.insert(attr.key.clone()) {
            return Err(ApiError::bad_request(format!(
                "Duplicate attribute key '{}'",
                attr.key
            )));
        }
        attr.kind = match attr.kind.as_str() {
            "double" => "float".into(),
            "bigint" => "integer".into(),
            other => other.to_lowercase(),
        };
        if !ATTRIBUTE_TYPES.contains(&attr.kind.as_str()) {
            return Err(ApiError::bad_request(format!(
                "Unsupported attribute type '{}' for '{}'",
                attr.kind, attr.key
            )));
        }
        if let Some(size) = attr.size {
            if size < 1 || size > 1_000_000 {
                return Err(ApiError::bad_request(format!(
                    "Attribute '{}' size must be between 1 and 1000000",
                    attr.key
                )));
            }
        }
        if let Some(elements) = &attr.elements {
            if elements.is_empty() {
                return Err(ApiError::bad_request(format!(
                    "Attribute '{}' enum needs at least one element",
                    attr.key
                )));
            }
        }
        if attr.kind == "enum" && attr.elements.is_none() {
            return Err(ApiError::bad_request(format!(
                "Attribute '{}' of type enum requires elements",
                attr.key
            )));
        }
        if let Some(default) = &attr.default {
            if !default.is_null() {
                validate_value(&attr, default, false)?;
            }
        }
        attrs.push(attr);
    }
    Ok(attrs)
}

pub fn validate_value(attr: &Attribute, value: &Value, is_create: bool) -> ApiResult<()> {
    if value.is_null() {
        if attr.required && is_create {
            return Err(ApiError::bad_request(format!(
                "Attribute '{}' is required and cannot be null",
                attr.key
            )));
        }
        if attr.required {
            return Err(ApiError::bad_request(format!(
                "Attribute '{}' is required and cannot be null",
                attr.key
            )));
        }
        return Ok(());
    }

    if attr.array {
        let Some(items) = value.as_array() else {
            return Err(ApiError::bad_request(format!(
                "Attribute '{}' must be an array",
                attr.key
            )));
        };
        if let Some(size) = attr.size {
            if items.len() as i64 > size {
                return Err(ApiError::bad_request(format!(
                    "Attribute '{}' allows at most {} items",
                    attr.key, size
                )));
            }
        }
        for item in items {
            validate_scalar(attr, item)?;
        }
        return Ok(());
    }

    validate_scalar(attr, value)
}

fn validate_scalar(attr: &Attribute, value: &Value) -> ApiResult<()> {
    let fail = |expected: &str| {
        ApiError::bad_request(format!(
            "Attribute '{}' must be a valid {}",
            attr.key, expected
        ))
    };
    match attr.kind.as_str() {
        "string" | "enum" | "email" | "url" | "ip" | "datetime" => {
            let Some(text) = value.as_str() else {
                return Err(fail("string"));
            };
            if let Some(size) = attr.size {
                if text.chars().count() as i64 > size {
                    return Err(ApiError::bad_request(format!(
                        "Attribute '{}' exceeds maximum size of {}",
                        attr.key, size
                    )));
                }
            }
            match attr.kind.as_str() {
                "email" => {
                    let parts: Vec<&str> = text.split('@').collect();
                    if parts.len() != 2
                        || parts[0].is_empty()
                        || !parts[1].contains('.')
                        || text.contains(char::is_whitespace)
                    {
                        return Err(fail("email address"));
                    }
                }
                "url" => {
                    if !(text.starts_with("http://") || text.starts_with("https://"))
                        || text.len() < 9
                    {
                        return Err(fail("URL"));
                    }
                }
                "ip" => {
                    if text.parse::<std::net::IpAddr>().is_err() {
                        return Err(fail("IP address"));
                    }
                }
                "datetime" => {
                    if DateTime::parse_from_rfc3339(text).is_err() {
                        return Err(fail("RFC 3339 datetime"));
                    }
                }
                "enum" => {
                    let allowed = attr.elements.as_deref().unwrap_or_default();
                    if !allowed.iter().any(|v| v == text) {
                        return Err(ApiError::bad_request(format!(
                            "Attribute '{}' must be one of: {}",
                            attr.key,
                            allowed.join(", ")
                        )));
                    }
                }
                _ => {}
            }
        }
        "integer" => {
            if value.as_i64().is_none() {
                return Err(fail("integer"));
            }
        }
        "float" => {
            if value.as_f64().is_none() {
                return Err(fail("number"));
            }
        }
        "boolean" => {
            if value.as_bool().is_none() {
                return Err(fail("boolean"));
            }
        }
        _ => return Err(ApiError::bad_request("Unknown attribute type")),
    }
    Ok(())
}

pub fn prepare_create(attrs: &[Attribute], data: Value) -> ApiResult<Value> {
    let Some(obj) = data.as_object() else {
        return Err(ApiError::bad_request("Document data must be an object"));
    };
    let mut out = Map::new();
    for key in obj.keys() {
        if key.starts_with('$') {
            return Err(ApiError::bad_request(format!(
                "System field '{key}' cannot be written"
            )));
        }
    }
    for attr in attrs {
        match obj.get(&attr.key) {
            Some(value) => {
                validate_value(attr, value, true)?;
                out.insert(attr.key.clone(), value.clone());
            }
            None => {
                if let Some(default) = &attr.default {
                    if !default.is_null() {
                        out.insert(attr.key.clone(), default.clone());
                        continue;
                    }
                }
                if attr.required {
                    return Err(ApiError::bad_request(format!(
                        "Missing required attribute '{}'",
                        attr.key
                    )));
                }
            }
        }
    }
    for key in obj.keys() {
        if !attrs.iter().any(|a| &a.key == key) {
            return Err(ApiError::bad_request(format!(
                "Unknown attribute '{key}'"
            )));
        }
    }
    Ok(Value::Object(out))
}

pub fn prepare_update(attrs: &[Attribute], data: Value) -> ApiResult<Value> {
    let Some(obj) = data.as_object() else {
        return Err(ApiError::bad_request("Document data must be an object"));
    };
    let mut out = Map::new();
    for (key, value) in obj {
        if key.starts_with('$') {
            return Err(ApiError::bad_request(format!(
                "System field '{key}' cannot be written"
            )));
        }
        let attr = attrs
            .iter()
            .find(|a| &a.key == key)
            .ok_or_else(|| ApiError::bad_request(format!("Unknown attribute '{key}'")))?;
        validate_value(attr, value, false)?;
        out.insert(key.clone(), value.clone());
    }
    Ok(Value::Object(out))
}

pub fn parse_permissions(value: Option<&Value>) -> ApiResult<Vec<String>> {
    let Some(value) = value else {
        return Ok(vec![]);
    };
    if value.is_null() {
        return Ok(vec![]);
    }
    let Some(items) = value.as_array() else {
        return Err(ApiError::bad_request("permissions must be an array"));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Some(text) = item.as_str() else {
            return Err(ApiError::bad_request(
                "Each permission must be a string like read(\"users\")",
            ));
        };
        parse_permission(text)
            .ok_or_else(|| ApiError::bad_request(format!("Invalid permission '{text}'")))?;
        out.push(text.to_string());
    }
    Ok(out)
}

fn parse_permission(text: &str) -> Option<(&str, &str)> {
    let (action, rest) = text.split_once("(\"")?;
    if !matches!(action, "read" | "create" | "update" | "delete" | "write") {
        return None;
    }
    let role = rest.strip_suffix("\")")?;
    if role.is_empty() || role.contains('"') || role.contains(')') || role.contains('\n') {
        return None;
    }
    Some((action, role))
}

fn role_matches(role: &str, user_id: Option<&str>) -> bool {
    match role {
        "any" => true,
        "guests" => user_id.is_none(),
        "users" => user_id.is_some(),
        role if role.starts_with("user:") => user_id
            .map(|id| role.strip_prefix("user:") == Some(id))
            .unwrap_or(false),
        // Teams, labels and memberships are intentionally unsupported in v1.
        _ => false,
    }
}

/// Returns true if any permission entry grants `action` to this viewer.
/// API keys are handled by callers and always bypass ACLs.
pub fn permission_allows(permissions: &[String], action: &str, user_id: Option<&str>) -> bool {
    permissions.iter().any(|entry| {
        let Some((entry_action, role)) = parse_permission(entry) else {
            return false;
        };
        let action_matches = entry_action == action || entry_action == "write";
        action_matches && role_matches(role, user_id)
    })
}


pub fn doc_default_permissions(user_id: Option<&str>) -> Vec<String> {
    match user_id {
        Some(id) => vec![
            format!("read(\"user:{id}\")"),
            format!("update(\"user:{id}\")"),
            format!("delete(\"user:{id}\")"),
        ],
        None => vec![],
    }
}

pub fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn query_error(message: impl Into<String>) -> ApiError {
    ApiError::new(
        axum::http::StatusCode::BAD_REQUEST,
        "general_query_invalid",
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(key: &str, kind: &str, required: bool) -> Attribute {
        Attribute {
            key: key.into(),
            kind: kind.into(),
            required,
            array: false,
            size: None,
            default: None,
            elements: None,
        }
    }

    #[test]
    fn required_and_unknown_fields_are_enforced() {
        let attrs = vec![attr("name", "string", true)];
        assert!(prepare_create(&attrs, json!({"name": "Ada"})).is_ok());
        assert!(prepare_create(&attrs, json!({})).is_err());
        assert!(prepare_create(&attrs, json!({"name": "Ada", "x": 1})).is_err());
    }

    #[test]
    fn type_validation_works() {
        let attrs = vec![attr("age", "integer", false), attr("live", "boolean", false)];
        assert!(prepare_create(&attrs, json!({"age": 42, "live": true})).is_ok());
        assert!(prepare_create(&attrs, json!({"age": 4.2})).is_err());
        assert!(prepare_create(&attrs, json!({"live": "yes"})).is_err());
    }

    #[test]
    fn permissions_are_applied() {
        let perms = vec!["read(\"users\")".to_string(), "update(\"user:u1\")".to_string()];
        assert!(permission_allows(&perms, "read", Some("u2")));
        assert!(!permission_allows(&perms, "update", Some("u2")));
        assert!(permission_allows(&perms, "update", Some("u1")));
        assert!(!permission_allows(&perms, "read", None));
        assert!(!permission_allows(&perms, "create", Some("u1")));
    }

    #[test]
    fn permission_validation_rejects_garbage() {
        assert!(parse_permissions(Some(&json!(["read(\"any\")"]))).is_ok());
        assert!(parse_permissions(Some(&json!(["read(any)"]))).is_err());
        assert!(parse_permissions(Some(&json!(["drop(\"any\")"]))).is_err());
    }
}

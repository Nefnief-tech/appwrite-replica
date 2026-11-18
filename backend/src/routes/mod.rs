pub mod account;
pub mod admin;
pub mod databases;
pub mod docker;
pub mod documents;
pub mod health;
pub mod import;
pub mod tablesdb;
pub mod users;

use axum::{
    Router,
    routing::{delete, get, patch, post, put},
};

use crate::realtime;

use crate::auth::AppState;

pub fn api_router() -> Router<AppState> {
    Router::new()        .route("/", get(health::root))
        .route("/version", get(health::version))
        .route("/v1/health", get(health::health))
        .route("/v1/health/version", get(health::version))
        .route("/v1/ping", get(health::ping))
        .route("/v1/realtime", get(realtime::realtime))
        .route("/v1/account", get(account::get_account))
        .route("/v1/account", post(account::create_account))
        .route("/v1/account/name", patch(account::update_name))
        .route("/v1/account/password", patch(account::update_password))
        .route("/v1/account/prefs", get(account::get_prefs))
        .route("/v1/account/prefs", patch(account::update_prefs))
        .route("/v1/account/recovery", post(account::create_recovery))
        .route("/v1/account/recovery", put(account::update_recovery))
        .route("/v1/account/verifications/email", post(account::create_verification))
        .route("/v1/account/verifications/email", put(account::update_verification))
        .route("/v1/account/mfa/factors", get(account::mfa_factors))
        .route("/v1/account/mfa", patch(account::mfa_update))
        .route("/v1/account/mfa/challenges", post(account::mfa_challenge_create))
        .route("/v1/account/mfa/challenges", put(account::mfa_challenge_update))
        .route("/v1/account/mfa/recovery-codes", get(account::mfa_recovery_codes_get))
        .route("/v1/account/mfa/recovery-codes", post(account::mfa_recovery_codes_create))
        .route("/v1/account/sessions", get(account::list_sessions))
        .route("/v1/account/sessions", delete(account::delete_sessions))
        .route("/v1/account/sessions/email", post(account::login))
        .route("/v1/account/jwts", post(account::create_jwt))
        .route(
            "/v1/account/sessions/{session_id}",
            get(account::get_session),
        )
        .route(
            "/v1/account/sessions/{session_id}",
            delete(account::delete_session),
        )
        .route("/v1/users", get(users::list_users))
        .route("/v1/users", post(users::create_user))
        .route("/v1/users/{user_id}", get(users::get_user))
        .route("/v1/users/{user_id}", delete(users::delete_user))
        .route("/v1/databases", get(databases::list_databases))
        .route("/v1/databases", post(databases::create_database))
        .route("/v1/databases/{database_id}", get(databases::get_database))
        .route(
            "/v1/databases/{database_id}",
            delete(databases::delete_database),
        )
        .route(
            "/v1/databases/{database_id}/collections",
            get(databases::list_collections),
        )
        .route(
            "/v1/databases/{database_id}/collections",
            post(databases::create_collection),
        )
        .route(
            "/v1/databases/{database_id}/collections/{collection_id}",
            get(databases::get_collection),
        )
        .route(
            "/v1/databases/{database_id}/collections/{collection_id}",
            delete(databases::delete_collection),
        )
        .route(
            "/v1/databases/{database_id}/collections/{collection_id}/attributes/{kind}",
            post(databases::create_attribute),
        )
        .route(
            "/v1/databases/{database_id}/collections/{collection_id}/attributes/{kind}",
            delete(databases::delete_attribute),
        )
        .route(
            "/v1/databases/{database_id}/collections/{collection_id}/documents",
            get(documents::list),
        )
        .route(
            "/v1/databases/{database_id}/collections/{collection_id}/documents",
            post(documents::create),
        )
        .route(
            "/v1/databases/{database_id}/collections/{collection_id}/documents/{document_id}",
            get(documents::get),
        )
        .route(
            "/v1/databases/{database_id}/collections/{collection_id}/documents/{document_id}",
            patch(documents::update),
        )
        .route(
            "/v1/databases/{database_id}/collections/{collection_id}/documents/{document_id}",
            delete(documents::delete),
        )
        .merge(tablesdb::router())
}

pub fn admin_router() -> Router<AppState> {
    Router::new()
        .route("/login", post(admin::login))
        .route("/logout", post(admin::logout))
        .route("/me", get(admin::me))
        .route("/overview", get(admin::overview))
        .route("/usage", get(admin::usage))
        .route("/nodes", get(admin::list_nodes))
        .route("/mail", get(admin::mail_settings))
        .route("/mail/templates", get(admin::list_mail_templates))
        .route("/mail/templates", put(admin::update_mail_template))
        .route("/nodes", post(admin::add_node))
        .route("/nodes/{node_id}", delete(admin::remove_node))
        .route("/nodes/{node_id}/check", post(admin::check_node))
        .route("/nodes/{node_id}/promote", post(admin::promote_node))
        .route("/docker/status", get(docker::status))
        .route("/docker/replicas", post(docker::spawn_replica))
        .route("/docker/replicas/{node_id}", delete(docker::remove_replica))
        .route("/projects", get(admin::list_projects))
        .route("/projects", post(admin::create_project))
        .route("/projects/{project_id}", get(admin::get_project))
        .route("/projects/{project_id}", delete(admin::delete_project))
        .route("/projects/{project_id}/key", post(admin::rotate_key))
        .route("/projects/{project_id}/users", get(admin::list_users))
        .route("/projects/{project_id}/users", post(admin::create_user))
        .route(
            "/projects/{project_id}/users/{user_id}",
            patch(admin::update_user),
        )
        .route(
            "/projects/{project_id}/users/{user_id}",
            delete(admin::delete_user),
        )
        .route(
            "/projects/{project_id}/databases",
            get(admin::list_databases),
        )
        .route(
            "/projects/{project_id}/databases",
            post(admin::create_database),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}",
            delete(admin::delete_database),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}/collections",
            get(admin::list_collections),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}/collections",
            post(admin::create_collection),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}/collections/{collection_id}",
            get(admin::get_collection),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}/collections/{collection_id}",
            patch(admin::update_collection),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}/collections/{collection_id}",
            delete(admin::delete_collection),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}/collections/{collection_id}/attributes/{kind}",
            post(admin::create_attribute),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}/collections/{collection_id}/attributes/{kind}",
            delete(admin::delete_attribute),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}/collections/{collection_id}/documents",
            get(admin::list_documents),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}/collections/{collection_id}/documents",
            post(admin::create_document),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}/collections/{collection_id}/documents/{document_id}",
            get(admin::get_document),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}/collections/{collection_id}/documents/{document_id}",
            patch(admin::update_document),
        )
        .route(
            "/projects/{project_id}/databases/{database_id}/collections/{collection_id}/documents/{document_id}",
            delete(admin::delete_document),
        )
        .route("/import", post(import::import_project))
}

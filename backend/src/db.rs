use anyhow::{Context, Result};
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::{
    config::Config,
    crypto::{hash_password, validate_resource_id},
};

pub async fn connect_control_plane(dsn: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(dsn)
        .await
        .context("could not connect to DATABASE_URL")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("failed to run control-plane migrations")?;

    Ok(pool)
}

pub async fn ensure_admin(pool: &PgPool, cfg: &Config) -> Result<()> {
    let existing: Option<String> =
        sqlx::query_scalar("select id::text from ow_admins where email = $1")
            .bind(cfg.admin_email.trim().to_lowercase())
            .fetch_optional(pool)
            .await?;
    if existing.is_some() {
        return Ok(());
    }

    if cfg.admin_password.len() < 10 {
        anyhow::bail!("ADMIN_PASSWORD must be at least 10 characters on first boot");
    }
    let hash = hash_password(cfg.admin_password.clone()).await?;
    sqlx::query(
        "insert into ow_admins (id, email, name, password_hash) values (gen_random_uuid(), $1, 'Admin', $2)",
    )
    .bind(cfg.admin_email.trim().to_lowercase())
    .bind(hash)
    .execute(pool)
    .await?;
    tracing::info!("bootstrap admin account created for {}", cfg.admin_email);
    Ok(())
}

pub fn schema_for_database(project_id: &str, database_id: &str) -> String {
    validate_resource_id(database_id, "Database ID").expect("validated database id");
    validate_resource_id(project_id, "Project ID").expect("validated project id");
    let raw = format!("ow_{project_id}_{database_id}");
    // Postgres identifiers max out at 63 bytes; long IDs get a stable hash.
    let name = if raw.len() <= 63 {
        raw
    } else {
        format!("ow_{}", &crate::crypto::sha256_hex(&raw)[..24])
    };
    crate::crypto::quote_ident(&name)
}

pub fn table_for_collection(collection_id: &str) -> String {
    validate_resource_id(collection_id, "Collection ID").expect("validated collection id");
    crate::crypto::quote_ident(collection_id)
}

pub fn physical_path(project_id: &str, database_id: &str, collection_id: &str) -> String {
    format!(
        "{}.{}",
        schema_for_database(project_id, database_id),
        table_for_collection(collection_id)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_are_scoped_to_the_project() {
        assert_eq!(schema_for_database("demo", "main"), "\"ow_demo_main\"");
        assert_ne!(
            schema_for_database("alpha", "main"),
            schema_for_database("beta", "main")
        );
    }

    #[test]
    fn long_schema_names_stay_within_the_identifier_limit() {
        let name = schema_for_database(&"p".repeat(36), &"d".repeat(36));
        assert!(name.len() <= 65, "{name}");
        assert_eq!(name, schema_for_database(&"p".repeat(36), &"d".repeat(36)));
    }
}

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use anyhow::{Context, Result};
use argon2::{
    Argon2,
    password_hash::{
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
    },
};
use base64::Engine;
use rand::{Rng, RngCore, distributions::Alphanumeric, thread_rng};
use sha2::{Digest, Sha256};

pub fn generate_id() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(20)
        .map(char::from)
        .collect()
}

pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn generate_api_key() -> String {
    format!("ow_{}", generate_token())
}

pub fn sha256_hex(input: &str) -> String {
    hex::encode(Sha256::digest(input.as_bytes()))
}

pub fn constant_time_eq_hex(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    let (Ok(a), Ok(b)) = (hex::decode(a), hex::decode(b)) else {
        return false;
    };
    a.ct_eq(&b).into()
}

pub fn validate_resource_id(id: &str, label: &str) -> Result<(), String> {
    let valid_len = !id.is_empty() && id.len() <= 36;
    let mut chars = id.chars();
    let starts_ok = chars
        .next()
        .map(|c| c.is_ascii_alphanumeric())
        .unwrap_or(false);
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if valid_len && starts_ok && rest_ok {
        Ok(())
    } else {
        Err(format!(
            "{label} must be 1-36 characters, start with a letter or number, and contain only letters, numbers, '.', '_' or '-'"
        ))
    }
}

pub fn validate_project_id(id: &str) -> Result<(), String> {
    let valid_len = !id.is_empty() && id.len() <= 36;
    let mut chars = id.chars();
    let starts_ok = chars
        .next()
        .map(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .unwrap_or(false);
    let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'));
    if valid_len && starts_ok && rest_ok {
        Ok(())
    } else {
        Err(
            "Project ID must be 1-36 lowercase letters, numbers, '-' or '_', starting with a letter or number"
                .into(),
        )
    }
}

pub fn validate_attribute_key(key: &str) -> Result<(), String> {
    let mut chars = key.chars();
    let starts_ok = chars
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false);
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if starts_ok && rest_ok && key.len() <= 64 {
        Ok(())
    } else {
        Err("Attribute key must start with a letter and contain only letters, numbers or underscores (max 64)".into())
    }
}

pub fn quote_ident(id: &str) -> String {
    format!("\"{}\"", id.replace('"', "\"\""))
}

pub async fn hash_password(password: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| anyhow::anyhow!("failed to hash password: {e}"))
    })
    .await?
}

pub async fn verify_password(password: String, hash: String) -> Result<bool> {
    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(&hash)
            .map_err(|err| anyhow::anyhow!("invalid password hash: {err}"))?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    })
    .await?
}

#[derive(Clone)]
pub struct NodeCipher {
    cipher: Aes256Gcm,
}

impl NodeCipher {
    pub fn new(secret: &str) -> Self {
        let key = Sha256::digest(secret.as_bytes());
        Self {
            cipher: Aes256Gcm::new_from_slice(&key).expect("32 byte key"),
        }
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let mut nonce_bytes = [0u8; 12];
        thread_rng().fill_bytes(&mut nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
            .map_err(|_| anyhow::anyhow!("failed to encrypt node DSN"))?;
        let mut combined = nonce_bytes.to_vec();
        combined.extend_from_slice(&ciphertext);
        Ok(base64::engine::general_purpose::STANDARD.encode(combined))
    }

    pub fn decrypt(&self, encoded: &str) -> Result<String> {
        let combined = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .context("invalid node DSN encoding")?;
        if combined.len() < 13 {
            anyhow::bail!("invalid node DSN payload");
        }
        let (nonce, ciphertext) = combined.split_at(12);
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| anyhow::anyhow!("failed to decrypt node DSN (was NODE_SECRET/SESSION_SECRET changed?)"))?;
        String::from_utf8(plaintext).context("node DSN is not valid UTF-8")
    }
}

pub fn validate_dsn(dsn: &str) -> Result<(), String> {
    if !(dsn.starts_with("postgres://") || dsn.starts_with("postgresql://")) {
        return Err("DSN must start with postgres:// or postgresql://".into());
    }
    if dsn.chars().any(char::is_whitespace) {
        return Err("DSN must not contain whitespace".into());
    }
    Ok(())
}

pub fn dsn_hint(dsn: &str) -> String {
    let Some((scheme, rest)) = dsn.split_once("://") else {
        return "invalid DSN".into();
    };
    let host = rest.split_once('@').map(|(_, h)| h).unwrap_or(rest);
    let host = host.split('?').next().unwrap_or(host);
    format!("{scheme}://{host}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_validated() {
        assert!(validate_resource_id("users-2024", "id").is_ok());
        assert!(validate_resource_id("-bad", "id").is_err());
        assert!(validate_resource_id("bad space", "id").is_err());
        assert!(validate_project_id("my-project").is_ok());
        assert!(validate_project_id("My-Project").is_err());
        assert!(validate_attribute_key("displayName").is_ok());
        assert!(validate_attribute_key("$internal").is_err());
    }

    #[test]
    fn dsn_is_masked() {
        let hint = dsn_hint("postgres://user:secret@db:5432/openwrite?sslmode=disable");
        assert_eq!(hint, "postgres://db:5432/openwrite");
        assert!(!hint.contains("secret"));
    }

    #[test]
    fn dsn_encrypts_roundtrip() {
        let cipher = NodeCipher::new("a-very-long-development-secret");
        let encrypted = cipher.encrypt("postgres://u:p@host/db").unwrap();
        assert!(!encrypted.contains("postgres"));
        assert_eq!(
            cipher.decrypt(&encrypted).unwrap(),
            "postgres://u:p@host/db"
        );
    }
}

use base64::{Engine as _, engine::general_purpose::STANDARD};

use ed25519_dalek::{Signer, SigningKey};

use sqlx::PgPool;
use std::{env, fs};
use uuid::Uuid;

pub struct ConfiguredSigningKey {
    pub key_id: String,
    signing_key: SigningKey,
}

fn load_private_key_base64(
    secret_file: Option<&str>,
    direct_value: Option<&str>,
) -> Result<String, String> {
    let value = match secret_file.filter(|path| !path.trim().is_empty()) {
        Some(path) => fs::read_to_string(path.trim())
            .map_err(|error| format!("Failed to read PATCHFORGE_PRIVATE_KEY_FILE: {error}"))?,
        None => direct_value
            .ok_or_else(|| {
                "PATCHFORGE_PRIVATE_KEY or PATCHFORGE_PRIVATE_KEY_FILE must be set".to_string()
            })?
            .to_string(),
    };
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err("The configured private key is empty".to_string());
    }
    Ok(value)
}

impl ConfiguredSigningKey {
    pub fn from_env() -> Result<Self, String> {
        let key_id = env::var("PATCHFORGE_KEY_ID")
            .map_err(|_| "PATCHFORGE_KEY_ID is not set".to_string())?;
        if key_id.trim().is_empty() || key_id.len() > 100 {
            return Err("PATCHFORGE_KEY_ID must contain 1 to 100 characters".to_string());
        }

        let secret_file = env::var("PATCHFORGE_PRIVATE_KEY_FILE").ok();
        let direct_value = env::var("PATCHFORGE_PRIVATE_KEY").ok();
        let private_key_base64 =
            load_private_key_base64(secret_file.as_deref(), direct_value.as_deref())?;
        let decoded = STANDARD
            .decode(private_key_base64)
            .map_err(|error| format!("Invalid private key Base64: {error}"))?;
        let key_bytes: [u8; 32] = decoded
            .try_into()
            .map_err(|_| "PATCHFORGE_PRIVATE_KEY must decode to exactly 32 bytes".to_string())?;

        Ok(Self {
            key_id,
            signing_key: SigningKey::from_bytes(&key_bytes),
        })
    }

    pub fn public_key_base64(&self) -> String {
        STANDARD.encode(self.signing_key.verifying_key().to_bytes())
    }

    pub fn sign(&self, payload: &str) -> String {
        STANDARD.encode(self.signing_key.sign(payload.as_bytes()).to_bytes())
    }
}

pub async fn sync_active_signing_key(pool: &PgPool) -> Result<(), String> {
    let key = ConfiguredSigningKey::from_env()?;
    let mut transaction = pool.begin().await.map_err(|error| error.to_string())?;

    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM signing_keys WHERE key_id = $1 FOR UPDATE")
            .bind(&key.key_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|error| error.to_string())?;
    if status.as_deref() == Some("revoked") {
        return Err(format!(
            "Configured signing key '{}' is revoked",
            key.key_id
        ));
    }

    sqlx::query(
        "UPDATE signing_keys SET status = 'retired', updated_at = NOW() WHERE status = 'active' AND key_id <> $1",
    )
    .bind(&key.key_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;

    sqlx::query(
        r#"
        INSERT INTO signing_keys (key_id, public_key_base64, status)
        VALUES ($1, $2, 'active')
        ON CONFLICT (key_id) DO UPDATE
        SET public_key_base64 = EXCLUDED.public_key_base64,
            status = 'active',
            updated_at = NOW()
        "#,
    )
    .bind(&key.key_id)
    .bind(key.public_key_base64())
    .execute(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;

    sqlx::query(
        r#"
        INSERT INTO audit_events (
            actor_fingerprint, method, resource_path, response_status
        )
        VALUES ('system', 'STARTUP', $1, 200)
        "#,
    )
    .bind(format!("/api/v1/signing-keys/{}/activate", key.key_id))
    .execute(&mut *transaction)
    .await
    .map_err(|error| error.to_string())?;

    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())
}

pub fn build_patch_payload(
    manifest_version: i32,
    key_id: &str,

    app_id: Uuid,
    release_id: Uuid,

    version: &str,
    platform: &str,

    patch_number: i32,

    artifact_path: &str,

    sha256: &str,
    file_size: i64,

    patch_format: Option<&str>,
    architecture: Option<&str>,

    base_sha256: Option<&str>,
    base_patch_number: Option<i32>,
    target_sha256: Option<&str>,
    target_file_size: Option<i64>,
    channel: &str,
    rollout_percentage: i32,
) -> String {
    let patch_format = patch_format.unwrap_or("");

    let architecture = architecture.unwrap_or("");

    let base_sha256 = base_sha256.unwrap_or("");

    let target_sha256 = target_sha256.unwrap_or("");

    let target_file_size = target_file_size
        .map(|value| value.to_string())
        .unwrap_or_default();

    let v1_payload = format!(
        "patchforge-manifest\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        manifest_version,
        key_id,
        app_id,
        release_id,
        version,
        platform,
        patch_number,
        artifact_path,
        sha256,
        file_size,
        patch_format,
        architecture,
        base_sha256,
        target_sha256,
        target_file_size,
    );

    if manifest_version == 1 {
        v1_payload
    } else {
        format!(
            "{}\n{}\n{}\n{}",
            v1_payload,
            base_patch_number
                .map(|value| value.to_string())
                .unwrap_or_default(),
            channel,
            rollout_percentage,
        )
    }
}

pub fn sign_payload(key: &ConfiguredSigningKey, payload: &str) -> String {
    key.sign(payload)
}

#[cfg(test)]
mod tests {
    use super::load_private_key_base64;
    use std::io::Write;

    #[test]
    fn secret_file_takes_precedence_and_is_trimmed() {
        let mut file = tempfile::NamedTempFile::new().expect("temporary secret file");
        writeln!(file, "file-secret").expect("write secret");
        let value = load_private_key_base64(file.path().to_str(), Some("stale-environment-secret"))
            .expect("secret value");
        assert_eq!(value, "file-secret");
    }

    #[test]
    fn missing_or_empty_secret_is_rejected() {
        assert!(load_private_key_base64(None, None).is_err());
        assert!(load_private_key_base64(None, Some("  ")).is_err());
    }
}

use axum::{Json, extract::Extension, extract::State, http::StatusCode};
use serde::Serialize;
use sqlx::PgPool;

use crate::routes::artifacts::ArtifactStorage;

#[derive(Debug, Serialize)]
pub struct SystemInfo {
    server_version: &'static str,
    database_provider: &'static str,
    storage: StorageInfo,
    signing: ActiveSigningKey,
    stats: SystemStats,
    supported_channels: [&'static str; 4],
    supported_architectures: [&'static str; 3],
}

#[derive(Debug, Serialize)]
struct StorageInfo {
    provider: &'static str,
    max_artifact_size_bytes: u64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct ActiveSigningKey {
    key_id: String,
    public_key_base64: String,
}

#[derive(Debug, Serialize)]
struct SystemStats {
    apps: i64,
    releases: i64,
    patches: i64,
    artifact_bytes: i64,
}

pub async fn system_info(
    State(pool): State<PgPool>,
    Extension(storage): Extension<ArtifactStorage>,
) -> Result<Json<SystemInfo>, StatusCode> {
    let signing = sqlx::query_as::<_, ActiveSigningKey>(
        r#"
        SELECT key_id, public_key_base64
        FROM signing_keys
        WHERE status = 'active'
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to read active signing key for system info: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let (apps, releases, patches, artifact_bytes) = tokio::try_join!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM apps").fetch_one(&pool),
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM releases").fetch_one(&pool),
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM patches").fetch_one(&pool),
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(file_size), 0)::BIGINT FROM patches",)
            .fetch_one(&pool),
    )
    .map_err(|error| {
        eprintln!("Failed to read system statistics: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(SystemInfo {
        server_version: env!("CARGO_PKG_VERSION"),
        database_provider: "postgresql",
        storage: StorageInfo {
            provider: storage.provider_name(),
            max_artifact_size_bytes: storage.max_size(),
        },
        signing,
        stats: SystemStats {
            apps,
            releases,
            patches,
            artifact_bytes,
        },
        supported_channels: ["development", "staging", "beta", "stable"],
        supported_architectures: ["arm64-v8a", "armeabi-v7a", "x86_64"],
    }))
}

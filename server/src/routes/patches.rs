use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    models::patch::{CreatePatchRequest, Patch},
    signing::{ConfiguredSigningKey, build_patch_payload, sign_payload},
};

#[derive(sqlx::FromRow)]
struct ReleaseSigningInfo {
    app_id: Uuid,
    version: String,
    platform: String,
    channel: String,
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_supported_architecture(value: &str) -> bool {
    matches!(value, "arm64-v8a" | "armeabi-v7a" | "x86_64")
}

pub async fn create_patch(
    Path(release_id): Path<Uuid>,
    State(pool): State<PgPool>,
    Json(payload): Json<CreatePatchRequest>,
) -> Result<(StatusCode, Json<Patch>), (StatusCode, String)> {
    // 1. Find release
    let release = sqlx::query_as::<_, ReleaseSigningInfo>(
        r#"
            SELECT
                app_id,
                version,
                platform,
                channel
            FROM releases
            WHERE id = $1
            "#,
    )
    .bind(release_id)
    .fetch_one(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to find release: {error}");

        (StatusCode::NOT_FOUND, "Release not found".to_string())
    })?;

    let architecture = payload
        .architecture
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "architecture is required".to_string(),
            )
        })?;
    let base_sha256 = payload.base_sha256.as_deref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "base_sha256 is required".to_string(),
        )
    })?;
    let base_patch_number = payload.base_patch_number.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "base_patch_number is required".to_string(),
        )
    })?;

    if !is_supported_architecture(architecture)
        || !matches!(payload.manifest_version, 1 | 2)
        || !(0..=100).contains(&payload.rollout_percentage)
        || payload.patch_format.as_deref() != Some("bsdiff")
        || !is_sha256(&payload.sha256)
        || !is_sha256(base_sha256)
        || !payload.target_sha256.as_deref().is_some_and(is_sha256)
        || payload.target_file_size.is_none_or(|size| size <= 0)
        || payload.file_size <= 0
        || payload.patch_number <= 0
        || base_patch_number < 0
        || base_patch_number >= payload.patch_number
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "A supported architecture and complete BSDIFF metadata with base_patch_number < patch_number are required".to_string(),
        ));
    }

    let expected_base_sha256: Option<String> = if base_patch_number == 0 {
        sqlx::query_scalar::<_, String>(
            r#"
            SELECT sha256
            FROM release_artifacts
            WHERE release_id = $1 AND architecture = $2
            "#,
        )
        .bind(release_id)
        .bind(architecture)
        .fetch_optional(&pool)
        .await
    } else {
        sqlx::query_scalar::<_, String>(
            r#"
            SELECT target_sha256
            FROM patches
            WHERE
                release_id = $1
                AND architecture = $2
                AND patch_number = $3
            "#,
        )
        .bind(release_id)
        .bind(architecture)
        .bind(base_patch_number)
        .fetch_optional(&pool)
        .await
    }
    .map_err(|error| {
        eprintln!("Failed to validate patch base: {error}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to validate patch compatibility".to_string(),
        )
    })?;

    let expected_base_sha256 = expected_base_sha256.ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            format!(
                "No compatible {} base artifact exists for patch {}",
                architecture, base_patch_number
            ),
        )
    })?;

    if !expected_base_sha256.eq_ignore_ascii_case(base_sha256) {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "Base SHA-256 mismatch for {} patch {}: expected {}, received {}",
                architecture, base_patch_number, expected_base_sha256, base_sha256
            ),
        ));
    }

    // 2. Read signing key ID
    let signing_key = ConfiguredSigningKey::from_env().map_err(|error| {
        eprintln!("Failed to load signing key: {error}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Signing key is not configured".to_string(),
        )
    })?;
    let key_id = signing_key.key_id.clone();
    let key_is_active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM signing_keys WHERE key_id = $1 AND status = 'active')",
    )
    .bind(&key_id)
    .fetch_one(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to validate active signing key: {error}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to validate signing key".to_string(),
        )
    })?;
    if !key_is_active {
        return Err((
            StatusCode::CONFLICT,
            "Configured signing key is not active".to_string(),
        ));
    }

    // 3. Build signed canonical payload
    let signing_payload = build_patch_payload(
        payload.manifest_version,
        &key_id,
        release.app_id,
        release_id,
        &release.version,
        &release.platform,
        payload.patch_number,
        &payload.file_path,
        &payload.sha256,
        payload.file_size,
        payload.patch_format.as_deref(),
        payload.architecture.as_deref(),
        payload.base_sha256.as_deref(),
        payload.base_patch_number,
        payload.target_sha256.as_deref(),
        payload.target_file_size,
        &release.channel,
        payload.rollout_percentage,
    );

    // 4. Sign manifest
    let signature = sign_payload(&signing_key, &signing_payload);

    // 5. Store patch
    let patch = sqlx::query_as::<_, Patch>(
        r#"
            INSERT INTO patches (
                release_id,
                patch_number,
                manifest_version,
                rollout_percentage,

                file_path,
                sha256,
                file_size,

                signature,
                key_id,

                patch_format,
                architecture,

                base_sha256,
                base_patch_number,
                target_sha256,
                target_file_size
            )
            VALUES (
                $1,
                $2,
                $3, $4, $5,
                $6, $7, $8, $9,
                $10, $11,
                $12, $13, $14, $15
            )

            RETURNING
                id,
                release_id,

                patch_number,
                manifest_version,
                rollout_percentage,

                file_path,
                sha256,
                file_size,

                signature,
                key_id,

                patch_format,
                architecture,

                base_sha256,
                base_patch_number,
                target_sha256,
                target_file_size,

                created_at
            "#,
    )
    .bind(release_id)
    .bind(payload.patch_number)
    .bind(payload.manifest_version)
    .bind(payload.rollout_percentage)
    .bind(payload.file_path)
    .bind(payload.sha256)
    .bind(payload.file_size)
    .bind(signature)
    .bind(key_id)
    .bind(payload.patch_format)
    .bind(payload.architecture)
    .bind(payload.base_sha256)
    .bind(payload.base_patch_number)
    .bind(payload.target_sha256)
    .bind(payload.target_file_size)
    .fetch_one(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to create patch: {error}");

        (
            StatusCode::CONFLICT,
            "A patch with this number and architecture already exists".to_string(),
        )
    })?;

    Ok((StatusCode::CREATED, Json(patch)))
}

pub async fn list_patches(
    Path(release_id): Path<Uuid>,
    State(pool): State<PgPool>,
) -> Result<Json<Vec<Patch>>, StatusCode> {
    let patches = sqlx::query_as::<_, Patch>(
        r#"
            SELECT
                id,
                release_id,

                patch_number,
                manifest_version,
                rollout_percentage,

                file_path,
                sha256,
                file_size,

                signature,
                key_id,

                patch_format,
                architecture,

                base_sha256,
                base_patch_number,
                target_sha256,
                target_file_size,

                created_at

            FROM patches

            WHERE release_id = $1

            ORDER BY
                patch_number DESC
            "#,
    )
    .bind(release_id)
    .fetch_all(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to list patches: {error}");

        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(patches))
}

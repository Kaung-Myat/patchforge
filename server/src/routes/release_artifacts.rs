use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::release_artifact::{CreateReleaseArtifactRequest, ReleaseArtifact};

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_supported_architecture(value: &str) -> bool {
    matches!(value, "arm64-v8a" | "armeabi-v7a" | "x86_64")
}

pub async fn create_release_artifact(
    Path(release_id): Path<Uuid>,
    State(pool): State<PgPool>,
    Json(payload): Json<CreateReleaseArtifactRequest>,
) -> Result<(StatusCode, Json<ReleaseArtifact>), (StatusCode, String)> {
    if !is_supported_architecture(&payload.architecture)
        || !is_sha256(&payload.sha256)
        || payload.file_size <= 0
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "a supported architecture, valid SHA-256, and positive file size are required"
                .to_string(),
        ));
    }

    let artifact = sqlx::query_as::<_, ReleaseArtifact>(
        r#"
        INSERT INTO release_artifacts (release_id, architecture, sha256, file_size)
        VALUES ($1, $2, $3, $4)
        RETURNING id, release_id, architecture, sha256, file_size, created_at
        "#,
    )
    .bind(release_id)
    .bind(payload.architecture)
    .bind(payload.sha256.to_ascii_lowercase())
    .bind(payload.file_size)
    .fetch_one(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to create release artifact: {error}");
        (
            StatusCode::CONFLICT,
            "A baseline artifact already exists for this release and architecture".to_string(),
        )
    })?;

    Ok((StatusCode::CREATED, Json(artifact)))
}

pub async fn list_release_artifacts(
    Path(release_id): Path<Uuid>,
    State(pool): State<PgPool>,
) -> Result<Json<Vec<ReleaseArtifact>>, StatusCode> {
    let artifacts = sqlx::query_as::<_, ReleaseArtifact>(
        r#"
        SELECT id, release_id, architecture, sha256, file_size, created_at
        FROM release_artifacts
        WHERE release_id = $1
        ORDER BY architecture
        "#,
    )
    .bind(release_id)
    .fetch_all(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to list release artifacts: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(artifacts))
}

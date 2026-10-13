use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::release::{CreateReleaseRequest, Release};

fn is_supported_channel(channel: &str) -> bool {
    matches!(channel, "development" | "staging" | "beta" | "stable")
}

pub async fn create_release(
    Path(app_id): Path<Uuid>,
    State(pool): State<PgPool>,
    Json(payload): Json<CreateReleaseRequest>,
) -> Result<(StatusCode, Json<Release>), (StatusCode, String)> {
    let version = payload.version.trim();
    if version.is_empty() || version.chars().count() > 50 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Release version must contain between 1 and 50 characters".to_string(),
        ));
    }
    if payload.platform != "android" {
        return Err((
            StatusCode::BAD_REQUEST,
            "Only the android platform is currently supported".to_string(),
        ));
    }
    if !is_supported_channel(&payload.channel) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Unsupported release channel".to_string(),
        ));
    }

    let release = sqlx::query_as::<_, Release>(
        r#"
        INSERT INTO releases (
            app_id,
            version,
            platform,
            channel
        )
        VALUES ($1, $2, $3, $4)
        RETURNING
            id,
            app_id,
            version,
            platform,
            channel,
            created_at
        "#,
    )
    .bind(app_id)
    .bind(version)
    .bind(payload.platform)
    .bind(payload.channel)
    .fetch_one(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to create release: {error}");
        if error
            .as_database_error()
            .is_some_and(|error| error.is_unique_violation())
        {
            (
                StatusCode::CONFLICT,
                "A release with this version, platform, and channel already exists".to_string(),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create release".to_string(),
            )
        }
    })?;

    Ok((StatusCode::CREATED, Json(release)))
}
pub async fn list_releases(
    Path(app_id): Path<Uuid>,
    State(pool): State<PgPool>,
) -> Result<Json<Vec<Release>>, StatusCode> {
    let releases = sqlx::query_as::<_, Release>(
        r#"
        SELECT
            id,
            app_id,
            version,
            platform,
            channel,
            created_at
        FROM releases
        WHERE app_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(app_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(releases))
}

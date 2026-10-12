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
) -> Result<(StatusCode, Json<Release>), StatusCode> {
    if !is_supported_channel(&payload.channel) {
        return Err(StatusCode::BAD_REQUEST);
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
    .bind(payload.version)
    .bind(payload.platform)
    .bind(payload.channel)
    .fetch_one(&pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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

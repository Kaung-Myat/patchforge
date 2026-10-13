use crate::models::app::{App, CreateAppRequest};
use axum::{Json, extract::State, http::StatusCode};
use sqlx::PgPool;

pub async fn create_app(
    State(pool): State<PgPool>,
    Json(payload): Json<CreateAppRequest>,
) -> Result<(StatusCode, Json<App>), (StatusCode, String)> {
    let name = payload.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Application name must contain between 1 and 100 characters".to_string(),
        ));
    }

    let app = sqlx::query_as::<_, App>(
        r#"
        INSERT INTO apps (name)
        VALUES ($1)
        RETURNING id, name, created_at
        "#,
    )
    .bind(name)
    .fetch_one(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to create application: {error}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create application".to_string(),
        )
    })?;

    Ok((StatusCode::CREATED, Json(app)))
}

pub async fn list_apps(State(pool): State<PgPool>) -> Result<Json<Vec<App>>, StatusCode> {
    let apps = sqlx::query_as::<_, App>(
        r#"
        SELECT id, name, created_at
        FROM apps
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(apps))
}

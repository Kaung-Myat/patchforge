use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use sqlx::PgPool;

use crate::models::signing_key::SigningKeyRecord;

pub async fn list_signing_keys(
    State(pool): State<PgPool>,
) -> Result<Json<Vec<SigningKeyRecord>>, StatusCode> {
    let keys = sqlx::query_as::<_, SigningKeyRecord>(
        r#"
        SELECT key_id, public_key_base64, status, created_at, updated_at
        FROM signing_keys
        ORDER BY created_at, key_id
        "#,
    )
    .fetch_all(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to list signing keys: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(keys))
}

pub async fn revoke_signing_key(
    Path(key_id): Path<String>,
    State(pool): State<PgPool>,
) -> Result<Json<SigningKeyRecord>, (StatusCode, String)> {
    let key = sqlx::query_as::<_, SigningKeyRecord>(
        r#"
        UPDATE signing_keys
        SET status = 'revoked', updated_at = NOW()
        WHERE key_id = $1 AND status <> 'active'
        RETURNING key_id, public_key_base64, status, created_at, updated_at
        "#,
    )
    .bind(&key_id)
    .fetch_optional(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to revoke signing key: {error}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to revoke signing key".to_string(),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            "Signing key does not exist or is still active".to_string(),
        )
    })?;
    Ok(Json(key))
}

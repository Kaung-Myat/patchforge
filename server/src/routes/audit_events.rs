use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use sqlx::PgPool;

use crate::models::audit_event::AuditEvent;

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    pub limit: Option<i64>,
    pub before_id: Option<i64>,
}

pub async fn list_audit_events(
    State(pool): State<PgPool>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEvent>>, StatusCode> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let events = sqlx::query_as::<_, AuditEvent>(
        r#"
        SELECT id, actor_fingerprint, method, resource_path, response_status, created_at
        FROM audit_events
        WHERE ($1::BIGINT IS NULL OR id < $1)
        ORDER BY id DESC
        LIMIT $2
        "#,
    )
    .bind(query.before_id)
    .bind(limit)
    .fetch_all(&pool)
    .await
    .map_err(|error| {
        eprintln!("Failed to list audit events: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(events))
}

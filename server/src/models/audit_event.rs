use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AuditEvent {
    pub id: i64,
    pub actor_fingerprint: String,
    pub method: String,
    pub resource_path: String,
    pub response_status: i32,
    pub created_at: DateTime<Utc>,
}

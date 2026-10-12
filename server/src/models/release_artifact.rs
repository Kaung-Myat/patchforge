use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ReleaseArtifact {
    pub id: Uuid,
    pub release_id: Uuid,
    pub architecture: String,
    pub sha256: String,
    pub file_size: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateReleaseArtifactRequest {
    pub architecture: String,
    pub sha256: String,
    pub file_size: i64,
}

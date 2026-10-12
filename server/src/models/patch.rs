use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Patch {
    pub id: Uuid,
    pub release_id: Uuid,

    pub patch_number: i32,
    pub manifest_version: i32,
    pub rollout_percentage: i32,

    // Downloadable patch artifact
    pub file_path: String,
    pub sha256: String,
    pub file_size: i64,

    // Signing
    pub signature: Option<String>,
    pub key_id: Option<String>,

    // Delta patch metadata
    pub patch_format: Option<String>,
    pub architecture: Option<String>,

    pub base_sha256: Option<String>,
    pub base_patch_number: Option<i32>,
    pub target_sha256: Option<String>,
    pub target_file_size: Option<i64>,

    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePatchRequest {
    pub patch_number: i32,
    #[serde(default = "default_manifest_version")]
    pub manifest_version: i32,
    #[serde(default = "default_rollout_percentage")]
    pub rollout_percentage: i32,

    pub file_path: String,
    pub sha256: String,
    pub file_size: i64,

    pub patch_format: Option<String>,
    pub architecture: Option<String>,

    pub base_sha256: Option<String>,
    pub base_patch_number: Option<i32>,
    pub target_sha256: Option<String>,
    pub target_file_size: Option<i64>,
}

fn default_manifest_version() -> i32 {
    1
}

fn default_rollout_percentage() -> i32 {
    100
}

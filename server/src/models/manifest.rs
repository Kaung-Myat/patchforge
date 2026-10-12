use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct PatchManifest {
    pub manifest_version: i32,
    pub key_id: String,

    pub app_id: Uuid,
    pub release_id: Uuid,

    pub version: String,
    pub platform: String,
    pub channel: String,

    pub patch_number: i32,

    // Downloadable delta artifact
    pub artifact_path: String,
    pub sha256: String,
    pub file_size: i64,

    // Delta information
    pub patch_format: Option<String>,
    pub architecture: Option<String>,

    pub base_sha256: Option<String>,
    pub base_patch_number: Option<i32>,
    pub target_sha256: Option<String>,
    pub target_file_size: Option<i64>,
    pub rollout_percentage: i32,

    pub signature: String,
}

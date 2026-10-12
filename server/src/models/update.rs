use serde::{Deserialize, Serialize};

use crate::models::manifest::PatchManifest;

#[derive(Debug, Deserialize)]
pub struct UpdateQuery {
    pub version: String,
    pub platform: String,
    pub patch: Option<i32>,
    pub architecture: Option<String>,
    #[serde(default = "default_channel")]
    pub channel: String,
    pub device_id: Option<String>,
}

fn default_channel() -> String {
    "stable".to_string()
}

#[derive(Debug, Serialize)]
pub struct UpdateResponse {
    pub update_available: bool,
    pub manifest: Option<PatchManifest>,
}

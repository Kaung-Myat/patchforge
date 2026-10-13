use std::{env, path::PathBuf};

pub struct Config {
    pub database_url: String,
    pub server_host: String,
    pub server_port: String,
    pub publish_token_hashes: String,
    pub storage_dir: PathBuf,
    pub max_artifact_size_bytes: u64,
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        Self {
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),

            server_host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),

            server_port: env::var("SERVER_PORT").unwrap_or_else(|_| "8080".to_string()),

            publish_token_hashes: env::var("PATCHFORGE_PUBLISH_TOKEN_HASHES")
                .expect("PATCHFORGE_PUBLISH_TOKEN_HASHES must be set"),

            storage_dir: env::var("PATCHFORGE_STORAGE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("../storage")),

            max_artifact_size_bytes: env::var("PATCHFORGE_MAX_ARTIFACT_SIZE_BYTES")
                .ok()
                .map(|value| {
                    value
                        .parse::<u64>()
                        .expect("PATCHFORGE_MAX_ARTIFACT_SIZE_BYTES must be a positive integer")
                })
                .unwrap_or(100 * 1024 * 1024),
        }
    }
}

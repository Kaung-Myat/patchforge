use std::env;

pub struct Config {
    pub database_url: String,
    pub server_host: String,
    pub server_port: String,
    pub publish_token_hashes: String,
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
        }
    }
}

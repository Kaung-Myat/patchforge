mod auth;
mod config;
mod db;
mod edge_security;
mod models;
mod routes;
mod signing;

use axum::{
    Router,
    extract::{DefaultBodyLimit, Extension},
    middleware,
    response::Redirect,
    routing::{get, post},
};

use auth::{PublishAuth, require_publisher};
use config::Config;
use edge_security::{EdgeSecurity, protect};
use std::net::SocketAddr;

use routes::{
    apps::{create_app, list_apps},
    artifacts::{ArtifactStorage, upload_artifact},
    audit_events::list_audit_events,
    console,
    health::health,
    patches::{create_patch, list_patches},
    release_artifacts::{create_release_artifact, list_release_artifacts},
    releases::{create_release, list_releases},
    signing_keys::{list_signing_keys, revoke_signing_key},
    system::system_info,
    updates::check_update,
};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    let config = Config::from_env();

    let db_pool = db::connect(&config.database_url).await;
    signing::sync_active_signing_key(&db_pool)
        .await
        .expect("Failed to synchronize active signing key");
    let publish_auth = PublishAuth::from_hash_list(&config.publish_token_hashes)
        .expect("Invalid PATCHFORGE_PUBLISH_TOKEN_HASHES configuration")
        .with_audit_pool(db_pool.clone());
    let edge_security = EdgeSecurity::from_env().expect("Invalid edge security configuration");
    let artifact_storage =
        ArtifactStorage::new(config.storage_dir.clone(), config.max_artifact_size_bytes)
            .await
            .expect("Failed to initialize artifact storage");

    println!("Connected to PostgreSQL");

    let management_routes = Router::new()
        .route("/api/v1/system", get(system_info))
        .route("/api/v1/apps", post(create_app).get(list_apps))
        .route(
            "/api/v1/apps/{app_id}/releases",
            post(create_release).get(list_releases),
        )
        .route(
            "/api/v1/releases/{release_id}/patches",
            post(create_patch).get(list_patches),
        )
        .route(
            "/api/v1/releases/{release_id}/artifacts",
            post(create_release_artifact).get(list_release_artifacts),
        )
        .route(
            "/api/v1/artifacts/{*path}",
            axum::routing::put(upload_artifact),
        )
        .route("/api/v1/signing-keys", get(list_signing_keys))
        .route("/api/v1/audit-events", get(list_audit_events))
        .route(
            "/api/v1/signing-keys/{key_id}/revoke",
            post(revoke_signing_key),
        )
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .route_layer(middleware::from_fn_with_state(
            publish_auth,
            require_publisher,
        ))
        .layer(Extension(artifact_storage));

    let app = Router::new()
        .route("/", get(root))
        .route("/console", get(console_redirect))
        .route("/console/", get(console::index))
        .route("/console/app.js", get(console::javascript))
        .route("/console/styles.css", get(console::stylesheet))
        .route("/health", get(health))
        .route("/api/v1/apps/{app_id}/updates", get(check_update))
        .nest_service("/artifacts", ServeDir::new(config.storage_dir))
        .merge(management_routes)
        .layer(middleware::from_fn_with_state(edge_security, protect))
        .with_state(db_pool);

    let address = format!("{}:{}", config.server_host, config.server_port);

    let listener = tokio::net::TcpListener::bind(&address).await.unwrap();

    println!("PatchForge server running on http://{}", address);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

async fn root() -> Redirect {
    Redirect::temporary("/console/")
}

async fn console_redirect() -> Redirect {
    Redirect::temporary("/console/")
}

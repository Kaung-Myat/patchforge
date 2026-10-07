use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();

    println!("PatchForge server running on http://localhost:8080");

    axum::serve(listener, app).await.unwrap();
}
async fn root() -> &'static str {
    "PatchForge OTA Server"
}

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok"
    }))
}

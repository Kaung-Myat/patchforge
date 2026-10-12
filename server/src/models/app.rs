use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Debug print လုပ်လို့ရ
// JSON ပြောင်းလို့ရ
// Database row ကနေ struct ပြောင်းလို့ရ
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct App {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAppRequest {
    pub name: String,
}

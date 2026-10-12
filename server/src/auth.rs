use axum::{
    Json,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use subtle::{Choice, ConstantTimeEq};

#[derive(Clone)]
pub struct PublishAuth {
    token_hashes: Vec<[u8; 32]>,
    audit_pool: Option<PgPool>,
}

impl PublishAuth {
    pub fn from_hash_list(value: &str) -> Result<Self, String> {
        let token_hashes = value
            .split(',')
            .map(str::trim)
            .filter(|hash| !hash.is_empty())
            .map(|hash| {
                let decoded = hex::decode(hash)
                    .map_err(|error| format!("Invalid publisher token hash: {error}"))?;
                decoded.try_into().map_err(|_| {
                    "Each publisher token hash must be a 64-character SHA-256 hex value".to_string()
                })
            })
            .collect::<Result<Vec<[u8; 32]>, String>>()?;

        if token_hashes.is_empty() {
            return Err("At least one publisher token hash is required".to_string());
        }

        Ok(Self {
            token_hashes,
            audit_pool: None,
        })
    }

    pub fn with_audit_pool(mut self, pool: PgPool) -> Self {
        self.audit_pool = Some(pool);
        self
    }

    fn authorizes(&self, token: &str) -> bool {
        let candidate: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let matches = self
            .token_hashes
            .iter()
            .fold(Choice::from(0), |result, expected| {
                result | expected.ct_eq(&candidate)
            });

        bool::from(matches)
    }

    fn fingerprint(&self, token: &str) -> Option<String> {
        self.authorizes(token).then(|| {
            let digest = hex::encode(Sha256::digest(token.as_bytes()));
            digest[..16].to_string()
        })
    }
}

fn bearer_token(request: &Request) -> Option<&str> {
    let value = request
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, token) = value.split_once(' ')?;

    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() && !token.contains(' '))
        .then_some(token)
}

pub async fn require_publisher(
    State(auth): State<PublishAuth>,
    request: Request,
    next: Next,
) -> Response {
    let token = bearer_token(&request);

    if let Some(actor_fingerprint) = token.and_then(|value| auth.fingerprint(value)) {
        let method = request.method().to_string();
        let resource_path = request.uri().path().to_string();
        let response = next.run(request).await;

        if let Some(pool) = auth.audit_pool {
            let response_status = i32::from(response.status().as_u16());
            tokio::spawn(async move {
                if let Err(error) = sqlx::query(
                    r#"
                    INSERT INTO audit_events (
                        actor_fingerprint, method, resource_path, response_status
                    )
                    VALUES ($1, $2, $3, $4)
                    "#,
                )
                .bind(actor_fingerprint)
                .bind(method)
                .bind(resource_path)
                .bind(response_status)
                .execute(&pool)
                .await
                {
                    eprintln!("Failed to record management audit event: {error}");
                }
            });
        }

        return response;
    }

    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            "Bearer realm=\"patchforge-publisher\"",
        )],
        Json(json!({
            "error": "publisher_authentication_required",
            "message": "A valid PatchForge publisher token is required."
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{PublishAuth, bearer_token};
    use axum::{body::Body, http::Request};

    const FIRST_HASH: &str = "8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8";
    const SECOND_HASH: &str = "f44e64e75f3948e9f73f8dfa94721c4ce8cbb4f265c4790c702b2d41cfbf2753";

    #[test]
    fn accepts_any_configured_rotation_token() {
        let auth = PublishAuth::from_hash_list(&format!("{FIRST_HASH},{SECOND_HASH}"))
            .expect("valid hashes");

        assert!(auth.authorizes("alpha"));
        assert!(auth.authorizes("beta"));
        assert!(!auth.authorizes("gamma"));
    }

    #[test]
    fn rejects_missing_or_invalid_hash_configuration() {
        assert!(PublishAuth::from_hash_list("").is_err());
        assert!(PublishAuth::from_hash_list("not-a-hash").is_err());
        assert!(PublishAuth::from_hash_list("00").is_err());
    }

    #[test]
    fn parses_bearer_scheme_case_insensitively_and_rejects_malformed_values() {
        let lowercase = Request::builder()
            .header("authorization", "bearer alpha")
            .body(Body::empty())
            .expect("request");
        let extra_segment = Request::builder()
            .header("authorization", "Bearer alpha beta")
            .body(Body::empty())
            .expect("request");
        let wrong_scheme = Request::builder()
            .header("authorization", "Basic alpha")
            .body(Body::empty())
            .expect("request");

        assert_eq!(bearer_token(&lowercase), Some("alpha"));
        assert_eq!(bearer_token(&extra_segment), None);
        assert_eq!(bearer_token(&wrong_scheme), None);
    }
}

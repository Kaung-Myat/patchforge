use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::{
    collections::HashMap,
    env,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct EdgeSecurity {
    trust_proxy: bool,
    https_enabled: bool,
    public_limit: u32,
    artifact_limit: u32,
    management_limit: u32,
    request_timeout: Duration,
    windows: Arc<Mutex<HashMap<(String, &'static str), RateWindow>>>,
}

struct RateWindow {
    started_at: Instant,
    count: u32,
}

impl EdgeSecurity {
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            trust_proxy: env_bool("PATCHFORGE_TRUST_PROXY", false)?,
            https_enabled: env_bool("PATCHFORGE_HTTPS_ENABLED", false)?,
            public_limit: env_u32("PATCHFORGE_PUBLIC_RATE_LIMIT_PER_MINUTE", 120)?,
            artifact_limit: env_u32("PATCHFORGE_ARTIFACT_RATE_LIMIT_PER_MINUTE", 300)?,
            management_limit: env_u32("PATCHFORGE_MANAGEMENT_RATE_LIMIT_PER_MINUTE", 60)?,
            request_timeout: Duration::from_secs(env_u64(
                "PATCHFORGE_REQUEST_TIMEOUT_SECONDS",
                120,
            )?),
            windows: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn allow(&self, client: &str, group: &'static str, limit: u32) -> bool {
        let mut windows = self.windows.lock().expect("rate-limit mutex poisoned");
        let now = Instant::now();
        if windows.len() > 10_000 {
            windows.retain(|_, window| {
                now.duration_since(window.started_at) < Duration::from_secs(120)
            });
        }
        let window = windows
            .entry((client.to_string(), group))
            .or_insert(RateWindow {
                started_at: now,
                count: 0,
            });
        if now.duration_since(window.started_at) >= Duration::from_secs(60) {
            window.started_at = now;
            window.count = 0;
        }
        if window.count >= limit {
            return false;
        }
        window.count += 1;
        true
    }

    fn client_id(&self, request: &Request) -> String {
        if self.trust_proxy
            && let Some(ip) = request
                .headers()
                .get("x-forwarded-for")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(',').next())
                .map(str::trim)
                .and_then(|value| value.parse::<IpAddr>().ok())
        {
            return ip.to_string();
        }
        request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|address| address.0.ip().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn rate_policy(&self, path: &str) -> Option<(&'static str, u32)> {
        if path.starts_with("/artifacts/") {
            Some(("artifact", self.artifact_limit))
        } else if path.starts_with("/api/v1/apps/") && path.ends_with("/updates") {
            Some(("public", self.public_limit))
        } else if path.starts_with("/api/v1/") {
            Some(("management", self.management_limit))
        } else {
            None
        }
    }

    fn secure_response(&self, response: &mut Response) {
        let headers = response.headers_mut();
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
        headers.insert(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        );
        headers.insert(
            "permissions-policy",
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        );
        headers.insert(
            "content-security-policy",
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
            ),
        );
        headers.insert(
            "cross-origin-opener-policy",
            HeaderValue::from_static("same-origin"),
        );
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        if self.https_enabled {
            headers.insert(
                header::STRICT_TRANSPORT_SECURITY,
                HeaderValue::from_static("max-age=31536000; includeSubDomains"),
            );
        }
    }
}

fn env_bool(name: &str, default: bool) -> Result<bool, String> {
    match env::var(name) {
        Ok(value) => value
            .parse::<bool>()
            .map_err(|_| format!("{name} must be true or false")),
        Err(_) => Ok(default),
    }
}

fn env_u32(name: &str, default: u32) -> Result<u32, String> {
    let value = env::var(name)
        .ok()
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|_| format!("{name} must be a positive integer"))?
        .unwrap_or(default);
    (value > 0)
        .then_some(value)
        .ok_or_else(|| format!("{name} must be greater than zero"))
}

fn env_u64(name: &str, default: u64) -> Result<u64, String> {
    let value = env::var(name)
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| format!("{name} must be a positive integer"))?
        .unwrap_or(default);
    (value > 0)
        .then_some(value)
        .ok_or_else(|| format!("{name} must be greater than zero"))
}

pub async fn protect(
    State(security): State<EdgeSecurity>,
    request: Request,
    next: Next,
) -> Response {
    let client = security.client_id(&request);
    if let Some((group, limit)) = security.rate_policy(request.uri().path())
        && !security.allow(&client, group, limit)
    {
        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "60")],
            Json(json!({
                "error": "rate_limit_exceeded",
                "message": "Too many requests. Retry after the current window."
            })),
        )
            .into_response();
        security.secure_response(&mut response);
        return response;
    }

    let mut response = match tokio::time::timeout(security.request_timeout, next.run(request)).await
    {
        Ok(response) => response,
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(json!({
                "error": "request_timeout",
                "message": "The request exceeded the configured time limit."
            })),
        )
            .into_response(),
    };
    security.secure_response(&mut response);
    response
}

#[cfg(test)]
mod tests {
    use super::EdgeSecurity;
    use axum::{body::Body, response::Response};
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    fn security() -> EdgeSecurity {
        EdgeSecurity {
            trust_proxy: false,
            https_enabled: false,
            public_limit: 2,
            artifact_limit: 3,
            management_limit: 1,
            request_timeout: Duration::from_secs(1),
            windows: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[test]
    fn applies_independent_route_policies() {
        let security = security();
        assert_eq!(
            security.rate_policy("/api/v1/apps/a/updates"),
            Some(("public", 2))
        );
        assert_eq!(
            security.rate_policy("/artifacts/patches/a.patch"),
            Some(("artifact", 3))
        );
        assert_eq!(
            security.rate_policy("/api/v1/signing-keys"),
            Some(("management", 1))
        );
        assert_eq!(security.rate_policy("/health"), None);
    }

    #[test]
    fn rejects_requests_after_the_limit() {
        let security = security();
        assert!(security.allow("127.0.0.1", "public", 2));
        assert!(security.allow("127.0.0.1", "public", 2));
        assert!(!security.allow("127.0.0.1", "public", 2));
    }

    #[test]
    fn adds_browser_security_headers() {
        let security = security();
        let mut response = Response::new(Body::empty());

        security.secure_response(&mut response);

        assert_eq!(
            response.headers()["content-security-policy"],
            "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"
        );
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        assert_eq!(
            response.headers()["cross-origin-opener-policy"],
            "same-origin"
        );
    }
}

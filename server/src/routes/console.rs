use axum::{http::header, response::IntoResponse};

const INDEX_HTML: &str = include_str!("../../console/index.html");
const APP_JS: &str = include_str!("../../console/app.js");
const STYLES_CSS: &str = include_str!("../../console/styles.css");

pub async fn index() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        INDEX_HTML,
    )
}

pub async fn javascript() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        APP_JS,
    )
}

pub async fn stylesheet() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        STYLES_CSS,
    )
}

#[cfg(test)]
mod tests {
    use super::{APP_JS, INDEX_HTML, STYLES_CSS};

    #[test]
    fn embeds_a_self_contained_console_without_remote_assets() {
        assert!(INDEX_HTML.contains("PatchForge Console"));
        assert!(INDEX_HTML.contains("/console/app.js"));
        assert!(INDEX_HTML.contains("/console/styles.css"));
        assert!(!INDEX_HTML.contains("src=\"http"));
        assert!(!INDEX_HTML.contains("href=\"http"));
        assert!(INDEX_HTML.contains("sidebarToggle"));
        assert!(INDEX_HTML.contains("activityChart"));
        assert!(APP_JS.contains("sessionStorage"));
        assert!(APP_JS.contains("createElementNS"));
        assert!(!APP_JS.contains("localStorage"));
        assert!(STYLES_CSS.contains("prefers-reduced-motion"));
        assert!(!STYLES_CSS.contains("gradient"));
        assert!(!STYLES_CSS.contains("box-shadow"));
    }
}

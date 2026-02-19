//! Unified dashboard handlers for Operate + Observe UX.

use axum::extract::Path;
use axum::http::header;
use axum::response::{Html, IntoResponse};

/// Serves the unified dashboard shell.
///
/// `GET /programs/{id}/dashboard`
pub async fn ui_index(Path(program_id): Path<i64>) -> Html<String> {
    let html = include_str!("../../static/dashboard/index.html")
        .replace("__PROGRAM_ID__", &program_id.to_string());
    Html(html)
}

/// Serves dashboard client JavaScript.
///
/// `GET /programs/{id}/dashboard/app.js`
pub async fn ui_app_js(Path(_program_id): Path<i64>) -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../../static/dashboard/app.js"),
    )
}

/// Serves dashboard client CSS.
///
/// `GET /programs/{id}/dashboard/styles.css`
pub async fn ui_styles_css(Path(_program_id): Path<i64>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../static/dashboard/styles.css"),
    )
}

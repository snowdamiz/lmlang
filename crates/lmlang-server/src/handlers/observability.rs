//! Human-observability handlers for graph visualization and NL query UX.

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{Html, IntoResponse};
use axum::Json;

use crate::error::ApiError;
use crate::schema::observability::{
    ObservabilityGraphRequest, ObservabilityGraphResponse, ObservabilityQueryRequest,
    ObservabilityQueryResponse,
};
use crate::state::AppState;

fn ensure_active_program(active: i64, requested: i64) -> Result<(), ApiError> {
    if active != requested {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            requested, active
        )));
    }
    Ok(())
}

/// Returns graph projection data for observability rendering.
///
/// `GET /programs/{id}/observability/graph`
pub async fn graph(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    Query(req): Query<ObservabilityGraphRequest>,
) -> Result<Json<ObservabilityGraphResponse>, ApiError> {
    let service = state.service.lock().await;
    ensure_active_program(service.program_id().0, program_id)?;
    let response = service.observability_graph(req)?;
    Ok(Json(response))
}

/// Runs a natural-language observability query with ambiguity/fallback behavior.
///
/// `POST /programs/{id}/observability/query`
pub async fn query(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    Json(req): Json<ObservabilityQueryRequest>,
) -> Result<Json<ObservabilityQueryResponse>, ApiError> {
    let service = state.service.lock().await;
    ensure_active_program(service.program_id().0, program_id)?;
    let response = service.observability_query(req)?;
    Ok(Json(response))
}

/// Serves the observability web UI shell.
///
/// `GET /programs/{id}/observability`
pub async fn ui_index(Path(program_id): Path<i64>) -> Html<String> {
    let html = include_str!("../../static/observability/index.html")
        .replace("__PROGRAM_ID__", &program_id.to_string());
    Html(html)
}

/// Serves observability client JavaScript.
///
/// `GET /programs/{id}/observability/app.js`
pub async fn ui_app_js(Path(_program_id): Path<i64>) -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../../static/observability/app.js"),
    )
}

/// Serves observability client CSS.
///
/// `GET /programs/{id}/observability/styles.css`
pub async fn ui_styles_css(Path(_program_id): Path<i64>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../static/observability/styles.css"),
    )
}

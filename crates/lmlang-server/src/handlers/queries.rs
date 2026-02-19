//! Query handlers for graph inspection.
//!
//! Provides endpoints for program overview, individual node/function lookups,
//! neighborhood traversal, and filtered search.

use axum::extract::{Path, Query, State};
use axum::Json;
use lmlang_core::id::{FunctionId, NodeId};
use serde::Deserialize;

use crate::error::ApiError;
use crate::schema::queries::{
    DetailLevel, GetFunctionResponse, NeighborhoodRequest, NeighborhoodResponse, NodeView,
    ProgramOverviewResponse, SearchRequest, SearchResponse,
};
use crate::state::AppState;

/// Query parameter for controlling response detail level.
#[derive(Debug, Clone, Deserialize)]
pub struct DetailQuery {
    /// Detail level: "summary", "standard", or "full".
    #[serde(default)]
    pub detail: Option<DetailLevel>,
}

/// Returns a high-level program overview.
///
/// `GET /programs/{id}/overview`
pub async fn program_overview(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
) -> Result<Json<ProgramOverviewResponse>, ApiError> {
    let service = state.service.lock().await;

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.program_overview()?;
    Ok(Json(response))
}

/// Returns a single node at the requested detail level.
///
/// `GET /programs/{id}/nodes/{node_id}`
pub async fn get_node(
    State(state): State<AppState>,
    Path((program_id, node_id)): Path<(i64, u32)>,
    Query(params): Query<DetailQuery>,
) -> Result<Json<NodeView>, ApiError> {
    let service = state.service.lock().await;

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let detail = params.detail.unwrap_or_default();
    let response = service.get_node(NodeId(node_id), detail)?;
    Ok(Json(response))
}

/// Returns a function with all its nodes and edges.
///
/// `GET /programs/{id}/functions/{func_id}`
pub async fn get_function(
    State(state): State<AppState>,
    Path((program_id, func_id)): Path<(i64, u32)>,
    Query(params): Query<DetailQuery>,
) -> Result<Json<GetFunctionResponse>, ApiError> {
    let service = state.service.lock().await;

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let detail = params.detail.unwrap_or_default();
    let response = service.get_function_context(FunctionId(func_id), detail)?;
    Ok(Json(response))
}

/// Returns the N-hop neighborhood around a node.
///
/// `POST /programs/{id}/neighborhood`
pub async fn neighborhood(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    Json(req): Json<NeighborhoodRequest>,
) -> Result<Json<NeighborhoodResponse>, ApiError> {
    let service = state.service.lock().await;

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let max_hops = req.max_hops.min(3);
    let response = service.get_neighborhood(req.node_id, max_hops, req.detail)?;
    Ok(Json(response))
}

/// Searches/filters nodes in the active program.
///
/// `POST /programs/{id}/search`
pub async fn search(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiError> {
    let service = state.service.lock().await;

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.search_nodes(req)?;
    Ok(Json(response))
}

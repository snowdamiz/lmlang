//! Router assembly for the lmlang HTTP API.
//!
//! [`build_router`] wires all handler functions to their routes with
//! CORS and tracing middleware layers.

use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::handlers;
use crate::state::AppState;

/// Builds the complete axum router with all API routes.
///
/// Routes use axum 0.8 `/{param}` path syntax.
/// CORS is permissive (agents may call from various origins).
/// TraceLayer provides request-level logging via tracing.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        // Agent management
        .route("/agents/register", post(handlers::agents::register_agent))
        .route("/agents/{agent_id}", delete(handlers::agents::deregister_agent))
        .route("/agents", get(handlers::agents::list_agents))
        // Program management
        .route(
            "/programs",
            get(handlers::programs::list_programs)
                .post(handlers::programs::create_program),
        )
        .route(
            "/programs/{id}",
            delete(handlers::programs::delete_program),
        )
        .route(
            "/programs/{id}/load",
            post(handlers::programs::load_program),
        )
        // Mutations (TOOL-01)
        .route(
            "/programs/{id}/mutations",
            post(handlers::mutations::propose_edit),
        )
        // Lock management
        .route(
            "/programs/{id}/locks/acquire",
            post(handlers::locks::acquire_locks),
        )
        .route(
            "/programs/{id}/locks/release",
            post(handlers::locks::release_locks),
        )
        .route("/programs/{id}/locks", get(handlers::locks::lock_status))
        // Queries (TOOL-02)
        .route(
            "/programs/{id}/overview",
            get(handlers::queries::program_overview),
        )
        .route(
            "/programs/{id}/nodes/{node_id}",
            get(handlers::queries::get_node),
        )
        .route(
            "/programs/{id}/functions/{func_id}",
            get(handlers::queries::get_function),
        )
        .route(
            "/programs/{id}/neighborhood",
            post(handlers::queries::neighborhood),
        )
        .route(
            "/programs/{id}/search",
            post(handlers::queries::search),
        )
        // Verify (TOOL-03)
        .route(
            "/programs/{id}/verify",
            post(handlers::verify::verify),
        )
        // Simulate (TOOL-04)
        .route(
            "/programs/{id}/simulate",
            post(handlers::simulate::simulate),
        )
        // Compile (EXEC-03/04)
        .route(
            "/programs/{id}/compile",
            post(handlers::compile::compile_program),
        )
        // Dirty status query (incremental compilation)
        .route(
            "/programs/{id}/dirty",
            get(handlers::compile::dirty_status),
        )
        // Property testing (CNTR-05)
        .route(
            "/programs/{id}/property-test",
            post(handlers::contracts::property_test),
        )
        // History (STORE-03)
        .route(
            "/programs/{id}/history",
            get(handlers::history::list_history),
        )
        .route(
            "/programs/{id}/undo",
            post(handlers::history::undo),
        )
        .route(
            "/programs/{id}/redo",
            post(handlers::history::redo),
        )
        .route(
            "/programs/{id}/checkpoints",
            get(handlers::history::list_checkpoints)
                .post(handlers::history::create_checkpoint),
        )
        .route(
            "/programs/{id}/checkpoints/{name}/restore",
            post(handlers::history::restore_checkpoint),
        )
        .route(
            "/programs/{id}/diff",
            post(handlers::history::diff_versions),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

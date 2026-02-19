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
        .route(
            "/agents/{agent_id}",
            get(handlers::agents::get_agent).delete(handlers::agents::deregister_agent),
        )
        .route(
            "/agents/{agent_id}/config",
            post(handlers::agents::update_agent_config),
        )
        .route("/agents", get(handlers::agents::list_agents))
        // Unified top-level dashboard
        .route("/dashboard", get(handlers::dashboard::ui_root_index))
        .route("/dashboard/ai/chat", post(handlers::dashboard::ai_chat))
        .route(
            "/dashboard/app.js",
            get(handlers::dashboard::ui_root_app_js),
        )
        .route(
            "/dashboard/styles.css",
            get(handlers::dashboard::ui_root_styles_css),
        )
        // Program management
        .route(
            "/programs",
            get(handlers::programs::list_programs).post(handlers::programs::create_program),
        )
        .route("/programs/{id}", delete(handlers::programs::delete_program))
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
        // Project-scoped agent control
        .route(
            "/programs/{id}/agents",
            get(handlers::agent_control::list_program_agents),
        )
        .route(
            "/programs/{id}/agents/{agent_id}/assign",
            post(handlers::agent_control::assign_program_agent),
        )
        .route(
            "/programs/{id}/agents/{agent_id}",
            get(handlers::agent_control::get_program_agent),
        )
        .route(
            "/programs/{id}/agents/{agent_id}/start",
            post(handlers::agent_control::start_program_agent),
        )
        .route(
            "/programs/{id}/agents/{agent_id}/stop",
            post(handlers::agent_control::stop_program_agent),
        )
        .route(
            "/programs/{id}/agents/{agent_id}/chat",
            post(handlers::agent_control::chat_with_program_agent),
        )
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
        .route("/programs/{id}/search", post(handlers::queries::search))
        .route(
            "/programs/{id}/semantic",
            post(handlers::queries::semantic_query),
        )
        // Observability (VIZ-01..VIZ-04)
        .route(
            "/programs/{id}/observability",
            get(handlers::observability::ui_index),
        )
        .route(
            "/programs/{id}/observability/app.js",
            get(handlers::observability::ui_app_js),
        )
        .route(
            "/programs/{id}/observability/styles.css",
            get(handlers::observability::ui_styles_css),
        )
        .route(
            "/programs/{id}/dashboard",
            get(handlers::dashboard::ui_index),
        )
        .route(
            "/programs/{id}/dashboard/app.js",
            get(handlers::dashboard::ui_app_js),
        )
        .route(
            "/programs/{id}/dashboard/styles.css",
            get(handlers::dashboard::ui_styles_css),
        )
        .route(
            "/programs/{id}/observability/graph",
            get(handlers::observability::graph),
        )
        .route(
            "/programs/{id}/observability/query",
            post(handlers::observability::query),
        )
        // Verify (TOOL-03)
        .route("/programs/{id}/verify", post(handlers::verify::verify))
        .route("/programs/{id}/verify/flush", post(handlers::verify::flush))
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
        .route("/programs/{id}/dirty", get(handlers::compile::dirty_status))
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
        .route("/programs/{id}/undo", post(handlers::history::undo))
        .route("/programs/{id}/redo", post(handlers::history::redo))
        .route(
            "/programs/{id}/checkpoints",
            get(handlers::history::list_checkpoints).post(handlers::history::create_checkpoint),
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

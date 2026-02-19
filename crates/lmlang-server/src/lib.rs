//! HTTP/JSON API server for AI agent interaction with lmlang program graphs.
//!
//! Provides a REST API that allows AI agents to build, query, verify, simulate,
//! and undo changes to program graphs. This crate contains the server framework,
//! API schema types, error handling, and route definitions.

pub mod agent_config_store;
pub mod autonomy_planner;
pub mod autonomous_runner;
pub mod concurrency;
pub mod error;
pub mod handlers;
pub mod llm_provider;
pub mod project_agent;
pub mod router;
pub mod schema;
pub mod service;
pub mod state;
pub mod undo;

pub use schema::autonomy_plan;

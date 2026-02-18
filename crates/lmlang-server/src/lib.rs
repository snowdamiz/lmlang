//! HTTP/JSON API server for AI agent interaction with lmlang program graphs.
//!
//! Provides a REST API that allows AI agents to build, query, verify, simulate,
//! and undo changes to program graphs. This crate contains the server framework,
//! API schema types, error handling, and route definitions.

pub mod error;
pub mod handlers;
pub mod schema;
pub mod service;
pub mod state;
pub mod undo;

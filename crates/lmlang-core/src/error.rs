//! Core error types for lmlang-core.
//!
//! Uses `thiserror` for structured, matchable error variants covering
//! all anticipated failure modes in the core graph data model.

use crate::id::{FunctionId, ModuleId, NodeId};
use crate::type_id::TypeId;
use thiserror::Error;

/// Core errors produced by the lmlang-core crate.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Attempting to register a type name that already exists in the registry.
    #[error("duplicate type name: '{name}'")]
    DuplicateTypeName { name: String },

    /// A TypeId was not found in the type registry.
    #[error("type not found: TypeId({id})", id = id.0)]
    TypeNotFound { id: TypeId },

    /// A node index was not found in the graph.
    #[error("node not found: NodeId({id})", id = id.0)]
    NodeNotFound { id: NodeId },

    /// A function ID was not found.
    #[error("function not found: FunctionId({id})", id = id.0)]
    FunctionNotFound { id: FunctionId },

    /// A module ID was not found.
    #[error("module not found: ModuleId({id})", id = id.0)]
    ModuleNotFound { id: ModuleId },

    /// An edge failed validation.
    #[error("invalid edge: {reason}")]
    InvalidEdge { reason: String },

    /// A dual-graph invariant was violated.
    #[error("graph inconsistency: {reason}")]
    GraphInconsistency { reason: String },
}

//! Mutation request/response types for graph editing operations.
//!
//! The agent submits mutations via [`ProposeEditRequest`], which supports
//! batch operations with all-or-nothing semantics. The `dry_run` flag
//! previews validation results without committing.

use lmlang_core::id::{EdgeId, FunctionId, ModuleId, NodeId};
use lmlang_core::ops::ComputeNodeOp;
use lmlang_core::type_id::TypeId;
use lmlang_core::types::Visibility;
use serde::{Deserialize, Serialize};

use super::diagnostics::{DiagnosticError, DiagnosticWarning};

/// Request to propose one or more graph mutations.
///
/// When `dry_run` is `true`, mutations are validated but not committed.
/// When `false`, mutations are validated and committed atomically (all or nothing).
#[derive(Debug, Clone, Deserialize)]
pub struct ProposeEditRequest {
    /// The mutations to apply.
    pub mutations: Vec<Mutation>,
    /// If `true`, validate only without committing.
    #[serde(default)]
    pub dry_run: bool,
}

/// A single graph mutation operation.
///
/// Each variant corresponds to one primitive graph edit. Batch mutations
/// are applied in order within a single atomic transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Mutation {
    /// Insert a new compute node.
    InsertNode {
        /// The operation this node performs.
        op: ComputeNodeOp,
        /// The function that owns this node.
        owner: FunctionId,
    },
    /// Remove a compute node and all its edges.
    RemoveNode {
        /// The node to remove.
        node_id: NodeId,
    },
    /// Change the operation of an existing node.
    ModifyNode {
        /// The node to modify.
        node_id: NodeId,
        /// The new operation.
        new_op: ComputeNodeOp,
    },
    /// Add a data flow edge between two nodes.
    AddEdge {
        /// Source node.
        from: NodeId,
        /// Target node.
        to: NodeId,
        /// Output port of the source node.
        source_port: u16,
        /// Input port of the target node.
        target_port: u16,
        /// Type of the value flowing through this edge.
        value_type: TypeId,
    },
    /// Add a control flow edge between two nodes.
    AddControlEdge {
        /// Source node (e.g., Branch).
        from: NodeId,
        /// Target node.
        to: NodeId,
        /// Branch index (0 = then, 1 = else, None = unconditional).
        branch_index: Option<u16>,
    },
    /// Remove an edge.
    RemoveEdge {
        /// The edge to remove.
        edge_id: EdgeId,
    },
    /// Add a new function definition.
    AddFunction {
        /// Function name.
        name: String,
        /// Owning module.
        module: ModuleId,
        /// Parameter names and types.
        params: Vec<(String, TypeId)>,
        /// Return type.
        return_type: TypeId,
        /// Visibility.
        visibility: Visibility,
    },
    /// Add a new module definition.
    AddModule {
        /// Module name.
        name: String,
        /// Parent module (None for top-level children of root).
        parent: Option<ModuleId>,
        /// Visibility.
        visibility: Visibility,
    },
}

/// Response from a propose-edit operation.
#[derive(Debug, Clone, Serialize)]
pub struct ProposeEditResponse {
    /// Whether all mutations passed validation.
    pub valid: bool,
    /// Entities created by the mutations.
    pub created: Vec<CreatedEntity>,
    /// Validation errors (non-empty if `valid` is `false`).
    pub errors: Vec<DiagnosticError>,
    /// Non-blocking warnings.
    pub warnings: Vec<DiagnosticWarning>,
    /// Whether the mutations were committed (`false` if `dry_run` or invalid).
    pub committed: bool,
}

/// An entity created by a mutation.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum CreatedEntity {
    /// A new node was created.
    Node { id: NodeId },
    /// A new edge was created.
    Edge { id: EdgeId },
    /// A new function was created.
    Function { id: FunctionId },
    /// A new module was created.
    Module { id: ModuleId },
}

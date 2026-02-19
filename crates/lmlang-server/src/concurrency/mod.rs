//! Multi-agent concurrency infrastructure.
//!
//! Provides the core building blocks for concurrent multi-agent editing:
//! - [`agent::AgentRegistry`] for session management
//! - [`lock_manager::LockManager`] for per-function read-write locks
//! - [`conflict`] for hash-based conflict detection

pub mod agent;
pub mod conflict;
pub mod lock_manager;
pub mod verify;

pub use agent::{AgentId, AgentRegistry, AgentSession};
pub use conflict::{check_hashes, build_function_diff, ConflictDetail, FunctionDiff};
pub use lock_manager::{
    FunctionLockState, LockDenial, LockError, LockGrant, LockHolderInfo, LockManager, LockMode,
    LockStatusEntry,
};

use std::collections::HashSet;

use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{FunctionId, NodeId};
use lmlang_core::ops::{ComputeNodeOp, ComputeOp};
use petgraph::graph::EdgeIndex;
use uuid::Uuid;

use crate::error::ApiError;
use crate::schema::mutations::Mutation;

/// Extracts the agent ID from the `X-Agent-Id` HTTP header.
///
/// Returns `ApiError::AgentRequired` if the header is missing or malformed.
/// Used by lock and agent-aware handlers in Plan 02.
pub fn extract_agent_id(headers: &axum::http::HeaderMap) -> Result<AgentId, ApiError> {
    headers
        .get("X-Agent-Id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .map(AgentId)
        .ok_or_else(|| ApiError::AgentRequired("X-Agent-Id header required".to_string()))
}

/// Computes which functions a batch of mutations touches.
///
/// Returns `(affected_functions, is_structure_change)`.
pub fn affected_functions(
    mutations: &[Mutation],
    graph: &ProgramGraph,
) -> (Vec<FunctionId>, bool) {
    let mut affected = HashSet::new();
    let mut structure_change = false;

    for mutation in mutations {
        match mutation {
            Mutation::InsertNode { owner, .. } => {
                affected.insert(*owner);
            }
            Mutation::RemoveNode { node_id } | Mutation::ModifyNode { node_id, .. } => {
                if let Some(node) = graph.get_compute_node(*node_id) {
                    affected.insert(node.owner);
                }
            }
            Mutation::AddEdge { from, to, .. } | Mutation::AddControlEdge { from, to, .. } => {
                add_owner_for_node(graph, &mut affected, *from);
                add_owner_for_node(graph, &mut affected, *to);
            }
            Mutation::RemoveEdge { edge_id } => {
                let edge_idx = EdgeIndex::<u32>::new(edge_id.0 as usize);
                if let Some((from, to)) = graph.compute().edge_endpoints(edge_idx) {
                    add_owner_for_node(graph, &mut affected, NodeId::from(from));
                    add_owner_for_node(graph, &mut affected, NodeId::from(to));
                }
            }
            Mutation::AddFunction { .. } | Mutation::AddModule { .. } => {
                structure_change = true;
            }
        }
    }

    let mut affected_list: Vec<FunctionId> = affected.into_iter().collect();
    affected_list.sort_by_key(|f| f.0);
    (affected_list, structure_change)
}

fn add_owner_for_node(
    graph: &ProgramGraph,
    affected: &mut HashSet<FunctionId>,
    node_id: NodeId,
) {
    if let Some(node) = graph.get_compute_node(node_id) {
        affected.insert(node.owner);
        if let ComputeNodeOp::Core(ComputeOp::Call { target }) = &node.op {
            affected.insert(*target);
        }
    }
}

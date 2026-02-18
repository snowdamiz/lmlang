//! Query request/response types for graph inspection.
//!
//! Provides types for querying nodes, edges, functions, and neighborhoods
//! with agent-controlled detail levels.

use lmlang_core::id::{EdgeId, FunctionId, ModuleId, NodeId};
use lmlang_core::ops::ComputeNodeOp;
use lmlang_core::type_id::TypeId;
use lmlang_core::types::Visibility;
use lmlang_storage::ProgramId;
use serde::{Deserialize, Serialize};

/// Controls the verbosity of query responses.
///
/// Agents select the level of detail based on their current task:
/// - `Summary`: IDs and basic metadata only (for orientation).
/// - `Standard`: Most fields populated (for typical operations).
/// - `Full`: Everything included, including edge lists on nodes.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum DetailLevel {
    /// Minimal: IDs and names only.
    Summary,
    /// Default level: most fields populated.
    #[default]
    Standard,
    /// Maximum detail: all fields including edge lists.
    Full,
}

/// A view of a compute node for API responses.
#[derive(Debug, Clone, Serialize)]
pub struct NodeView {
    /// Node identifier.
    pub id: NodeId,
    /// The operation this node performs.
    pub op: ComputeNodeOp,
    /// The function that owns this node.
    pub owner: FunctionId,
    /// Serialized op data for agent consumption.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op_data: Option<serde_json::Value>,
    /// Incoming edge IDs (populated at Standard+ detail).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incoming_edges: Option<Vec<EdgeId>>,
    /// Outgoing edge IDs (populated at Full detail).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outgoing_edges: Option<Vec<EdgeId>>,
}

/// A view of an edge for API responses.
#[derive(Debug, Clone, Serialize)]
pub struct EdgeView {
    /// Edge identifier.
    pub id: EdgeId,
    /// Source node.
    pub from: NodeId,
    /// Target node.
    pub to: NodeId,
    /// Edge kind: "data" or "control".
    pub kind: String,
    /// Value type for data edges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_type: Option<TypeId>,
    /// Source port for data edges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_port: Option<u16>,
    /// Target port for data edges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_port: Option<u16>,
    /// Branch index for control edges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_index: Option<u16>,
}

/// A view of a function for API responses.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionView {
    /// Function identifier.
    pub id: FunctionId,
    /// Function name.
    pub name: String,
    /// Owning module.
    pub module: ModuleId,
    /// Parameter names and types.
    pub params: Vec<(String, TypeId)>,
    /// Return type.
    pub return_type: TypeId,
    /// Visibility.
    pub visibility: Visibility,
    /// Whether this is a closure.
    pub is_closure: bool,
    /// Number of compute nodes in this function.
    pub node_count: usize,
}

/// High-level program overview response.
#[derive(Debug, Clone, Serialize)]
pub struct ProgramOverviewResponse {
    /// Program identifier.
    pub program_id: ProgramId,
    /// Program name.
    pub name: String,
    /// Module IDs in the program.
    pub modules: Vec<ModuleId>,
    /// Function IDs in the program.
    pub functions: Vec<FunctionId>,
    /// Total number of compute nodes.
    pub node_count: usize,
    /// Total number of edges.
    pub edge_count: usize,
}

/// Response for getting a function and its contents.
#[derive(Debug, Clone, Serialize)]
pub struct GetFunctionResponse {
    /// The function metadata.
    pub function: FunctionView,
    /// All compute nodes in this function.
    pub nodes: Vec<NodeView>,
    /// All edges between nodes in this function.
    pub edges: Vec<EdgeView>,
}

/// Request for a neighborhood query around a node.
#[derive(Debug, Clone, Deserialize)]
pub struct NeighborhoodRequest {
    /// Center node for the neighborhood.
    pub node_id: NodeId,
    /// Maximum hops from center (capped at 3).
    #[serde(default = "default_max_hops")]
    pub max_hops: u32,
    /// Response detail level.
    #[serde(default)]
    pub detail: DetailLevel,
}

fn default_max_hops() -> u32 {
    1
}

/// Response for a neighborhood query.
#[derive(Debug, Clone, Serialize)]
pub struct NeighborhoodResponse {
    /// The center node of the query.
    pub center: NodeId,
    /// All nodes within the hop radius.
    pub nodes: Vec<NodeView>,
    /// All edges between the returned nodes.
    pub edges: Vec<EdgeView>,
    /// Actual number of hops used (may be less than requested).
    pub hops_used: u32,
}

/// Request for searching/filtering nodes.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchRequest {
    /// Filter by op type name (e.g., "Const", "BinaryArith").
    #[serde(default)]
    pub filter_type: Option<String>,
    /// Filter by owning function.
    #[serde(default)]
    pub owner_function: Option<FunctionId>,
    /// Filter by value type on connected edges.
    #[serde(default)]
    pub value_type: Option<TypeId>,
    /// Response detail level.
    #[serde(default)]
    pub detail: DetailLevel,
}

/// Response for a node search.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResponse {
    /// Matching nodes.
    pub nodes: Vec<NodeView>,
    /// Total number of matches.
    pub total_count: usize,
}

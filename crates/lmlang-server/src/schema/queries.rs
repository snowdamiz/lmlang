//! Query request/response types for graph inspection.
//!
//! Provides types for querying nodes, edges, functions, and neighborhoods
//! with agent-controlled detail levels.

use lmlang_core::edge::SemanticEdge;
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

/// Request for semantic graph retrieval.
#[derive(Debug, Clone, Deserialize)]
pub struct SemanticQueryRequest {
    /// Whether embedding vectors should be included inline.
    #[serde(default)]
    pub include_embeddings: bool,
}

/// Ownership metadata in semantic query responses.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticOwnershipView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<ModuleId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

/// Provenance metadata in semantic query responses.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticProvenanceView {
    pub source: String,
    pub version: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Semantic node projection for retrieval workflows.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticNodeView {
    pub id: u32,
    pub kind: String,
    pub label: String,
    pub ownership: SemanticOwnershipView,
    pub provenance: SemanticProvenanceView,
    pub summary_title: String,
    pub summary_body: String,
    pub summary_checksum: String,
    pub token_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity: Option<u32>,
    pub has_node_embedding: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_embedding_dim: Option<usize>,
    pub has_subgraph_summary_embedding: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subgraph_summary_embedding_dim: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_embedding: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subgraph_summary_embedding: Option<Vec<f32>>,
}

/// Semantic edge projection.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticEdgeView {
    pub id: u32,
    pub from: u32,
    pub to: u32,
    pub relationship: SemanticEdge,
}

/// Semantic query response.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticQueryResponse {
    pub nodes: Vec<SemanticNodeView>,
    pub edges: Vec<SemanticEdgeView>,
    pub node_count: usize,
    pub edge_count: usize,
}

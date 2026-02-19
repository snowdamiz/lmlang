//! Observability request/response schema for human-facing graph exploration.
//!
//! This module defines DTOs for:
//! - dual-layer graph projection payloads (semantic + compute + cross-layer)
//! - natural-language observability query request/response contracts

use lmlang_core::edge::SemanticEdge;
use lmlang_core::id::{FunctionId, ModuleId, NodeId};
use lmlang_core::type_id::TypeId;
use serde::{Deserialize, Serialize};

/// Layer identity for observability payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservabilityLayer {
    Semantic,
    Compute,
}

/// Graph presets used by the UI for quick filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ObservabilityPreset {
    #[default]
    All,
    SemanticOnly,
    ComputeOnly,
    Interop,
}

fn default_true() -> bool {
    true
}

/// Query parameters for graph projection requests.
#[derive(Debug, Clone, Deserialize)]
pub struct ObservabilityGraphRequest {
    #[serde(default)]
    pub preset: ObservabilityPreset,
    #[serde(default = "default_true")]
    pub include_cross_layer: bool,
}

/// Function-boundary grouping metadata for graph rendering.
#[derive(Debug, Clone, Serialize)]
pub struct ObservabilityGroupView {
    pub id: String,
    pub function_id: FunctionId,
    pub function_name: String,
    pub module_id: ModuleId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_anchor_id: Option<String>,
    pub compute_node_ids: Vec<String>,
}

/// A projected node for observability visualization.
#[derive(Debug, Clone, Serialize)]
pub struct ObservabilityNodeView {
    pub id: String,
    pub layer: ObservabilityLayer,
    pub kind: String,
    pub label: String,
    pub short_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_id: Option<FunctionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_id: Option<ModuleId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_node_id: Option<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_node_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// A projected edge for observability visualization.
#[derive(Debug, Clone, Serialize)]
pub struct ObservabilityEdgeView {
    pub id: String,
    pub from: String,
    pub to: String,
    pub from_layer: ObservabilityLayer,
    pub to_layer: ObservabilityLayer,
    pub edge_kind: String,
    pub cross_layer: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_type: Option<TypeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_index: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<SemanticEdge>,
}

/// Full graph payload consumed by the observability UI.
#[derive(Debug, Clone, Serialize)]
pub struct ObservabilityGraphResponse {
    pub preset: ObservabilityPreset,
    pub node_count: usize,
    pub edge_count: usize,
    pub nodes: Vec<ObservabilityNodeView>,
    pub edges: Vec<ObservabilityEdgeView>,
    pub groups: Vec<ObservabilityGroupView>,
}

fn default_max_results() -> usize {
    5
}

/// Request payload for natural-language observability queries.
#[derive(Debug, Clone, Deserialize)]
pub struct ObservabilityQueryRequest {
    pub query: String,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    /// If set, disambiguates using a candidate id from `interpretations`.
    #[serde(default)]
    pub selected_candidate_id: Option<String>,
}

/// Suggested prompt chips shown in the UI.
#[derive(Debug, Clone, Serialize)]
pub struct SuggestedPromptChipView {
    pub id: String,
    pub label: String,
    pub query: String,
}

/// Ranked interpretation candidate for ambiguous queries.
#[derive(Debug, Clone, Serialize)]
pub struct QueryInterpretationView {
    pub candidate_id: String,
    pub node_id: String,
    pub label: String,
    pub score: f32,
    pub reason: String,
}

/// Summary tab payload.
#[derive(Debug, Clone, Serialize)]
pub struct QuerySummaryTabView {
    pub title: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_id: Option<ModuleId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_id: Option<FunctionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity: Option<u32>,
}

/// Relationship entry for the Relationships tab.
#[derive(Debug, Clone, Serialize)]
pub struct QueryRelationshipItemView {
    pub direction: String,
    pub edge_kind: String,
    pub node_id: String,
    pub label: String,
}

/// Relationships tab payload.
#[derive(Debug, Clone, Serialize)]
pub struct QueryRelationshipsTabView {
    pub mini_graph_node_ids: Vec<String>,
    pub mini_graph_edge_ids: Vec<String>,
    pub items: Vec<QueryRelationshipItemView>,
}

/// Contract entry in the Contracts tab.
#[derive(Debug, Clone, Serialize)]
pub struct QueryContractEntryView {
    pub node_id: String,
    pub contract_kind: String,
    pub message: String,
}

/// Contracts tab payload.
#[derive(Debug, Clone, Serialize)]
pub struct QueryContractsTabView {
    pub has_contracts: bool,
    pub entries: Vec<QueryContractEntryView>,
}

/// A ranked observability query result with contextual tabs.
#[derive(Debug, Clone, Serialize)]
pub struct ObservabilityQueryResultView {
    pub rank: usize,
    pub node_id: String,
    pub label: String,
    pub layer: ObservabilityLayer,
    pub score: f32,
    pub related_node_ids: Vec<String>,
    pub summary: QuerySummaryTabView,
    pub relationships: QueryRelationshipsTabView,
    pub contracts: QueryContractsTabView,
}

/// Natural-language observability query response.
#[derive(Debug, Clone, Serialize)]
pub struct ObservabilityQueryResponse {
    pub query: String,
    pub suggested_prompts: Vec<SuggestedPromptChipView>,
    pub ambiguous: bool,
    pub low_confidence: bool,
    pub confidence: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambiguity_prompt: Option<String>,
    pub interpretations: Vec<QueryInterpretationView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub results: Vec<ObservabilityQueryResultView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_graph_node_id: Option<String>,
}

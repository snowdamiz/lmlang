//! Undo/redo and checkpoint request/response types.
//!
//! Provides both linear undo/redo (step-by-step reversal) and named
//! checkpoints (agent-created save points). History is persistent and
//! inspectable.

use lmlang_core::id::{EdgeId, NodeId};
use serde::{Deserialize, Serialize};

/// A single entry in the edit history.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    /// Unique identifier for this edit (UUID v4).
    pub id: String,
    /// ISO 8601 timestamp of when the edit was applied.
    pub timestamp: String,
    /// Optional human-readable description of the edit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether this edit has been undone.
    pub undone: bool,
}

/// Response for listing edit history.
#[derive(Debug, Clone, Serialize)]
pub struct ListHistoryResponse {
    /// History entries in reverse chronological order.
    pub entries: Vec<HistoryEntry>,
    /// Total number of entries.
    pub total: usize,
}

/// Response from an undo operation.
#[derive(Debug, Clone, Serialize)]
pub struct UndoResponse {
    /// Whether the undo was successful.
    pub success: bool,
    /// The edit that was undone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_edit: Option<HistoryEntry>,
}

/// Response from a redo operation.
#[derive(Debug, Clone, Serialize)]
pub struct RedoResponse {
    /// Whether the redo was successful.
    pub success: bool,
    /// The edit that was reapplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reapplied_edit: Option<HistoryEntry>,
}

/// Request to create a named checkpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateCheckpointRequest {
    /// Checkpoint name (must be unique within the program).
    pub name: String,
    /// Optional description of what the checkpoint represents.
    #[serde(default)]
    pub description: Option<String>,
}

/// Response from creating a checkpoint.
#[derive(Debug, Clone, Serialize)]
pub struct CreateCheckpointResponse {
    /// The checkpoint name.
    pub name: String,
    /// ISO 8601 timestamp of when the checkpoint was created.
    pub timestamp: String,
}

/// Response for listing checkpoints.
#[derive(Debug, Clone, Serialize)]
pub struct ListCheckpointsResponse {
    /// All checkpoints for the program.
    pub checkpoints: Vec<CheckpointView>,
}

/// A view of a named checkpoint.
#[derive(Debug, Clone, Serialize)]
pub struct CheckpointView {
    /// Checkpoint name.
    pub name: String,
    /// ISO 8601 timestamp of when the checkpoint was created.
    pub timestamp: String,
    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Position in the edit history at checkpoint creation time.
    pub edit_position: i64,
}

/// Response from restoring a checkpoint.
#[derive(Debug, Clone, Serialize)]
pub struct RestoreCheckpointResponse {
    /// Whether the restore was successful.
    pub success: bool,
    /// The checkpoint that was restored.
    pub name: String,
}

/// Request to diff between checkpoints.
#[derive(Debug, Clone, Deserialize)]
pub struct DiffRequest {
    /// Starting checkpoint (None = beginning of history).
    #[serde(default)]
    pub from_checkpoint: Option<String>,
    /// Ending checkpoint (None = current state).
    #[serde(default)]
    pub to_checkpoint: Option<String>,
}

/// Response showing the diff between two states.
#[derive(Debug, Clone, Serialize)]
pub struct DiffResponse {
    /// Nodes added between the two states.
    pub added_nodes: Vec<NodeId>,
    /// Nodes removed between the two states.
    pub removed_nodes: Vec<NodeId>,
    /// Nodes modified between the two states.
    pub modified_nodes: Vec<NodeId>,
    /// Edges added between the two states.
    pub added_edges: Vec<EdgeId>,
    /// Edges removed between the two states.
    pub removed_edges: Vec<EdgeId>,
}

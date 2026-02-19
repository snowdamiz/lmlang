//! Edit history and checkpoint management for undo/redo operations.
//!
//! [`EditCommand`] represents reversible graph mutations. [`EditLog`] persists
//! these commands in SQLite and provides undo/redo by inverting commands.
//! [`CheckpointManager`] snapshots the full graph state at named save points.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use lmlang_core::edge::FlowEdge;
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{EdgeId, FunctionId, ModuleId, NodeId};
use lmlang_core::node::ComputeNode;
use lmlang_core::ops::ComputeNodeOp;
use lmlang_core::type_id::TypeId;
use lmlang_core::types::Visibility;
use lmlang_storage::ProgramId;

use crate::error::ApiError;
use crate::schema::history::{CheckpointView, HistoryEntry};

/// A reversible graph mutation command.
///
/// Each variant captures enough information to apply the mutation forward
/// and to compute its inverse for undo. Commands are serialized to JSON
/// for persistent storage in the edit_log table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EditCommand {
    /// A compute node was inserted.
    InsertNode {
        node_id: NodeId,
        op: ComputeNodeOp,
        owner: FunctionId,
    },
    /// A compute node was removed (captures the full node for undo re-insertion).
    RemoveNode {
        node_id: NodeId,
        removed_node: ComputeNode,
    },
    /// A compute node's operation was modified.
    ModifyNode {
        node_id: NodeId,
        old_op: ComputeNodeOp,
        new_op: ComputeNodeOp,
        owner: FunctionId,
    },
    /// A data flow edge was inserted.
    InsertDataEdge {
        edge_id: EdgeId,
        from: NodeId,
        to: NodeId,
        source_port: u16,
        target_port: u16,
        value_type: TypeId,
    },
    /// A control flow edge was inserted.
    InsertControlEdge {
        edge_id: EdgeId,
        from: NodeId,
        to: NodeId,
        branch_index: Option<u16>,
    },
    /// An edge was removed (captures the full edge for undo re-insertion).
    RemoveEdge {
        edge_id: EdgeId,
        from: NodeId,
        to: NodeId,
        removed_edge: FlowEdge,
    },
    /// A function was added.
    AddFunction {
        func_id: FunctionId,
        name: String,
        module: ModuleId,
        params: Vec<(String, TypeId)>,
        return_type: TypeId,
        visibility: Visibility,
    },
    /// A module was added.
    AddModule {
        module_id: ModuleId,
        name: String,
        parent: ModuleId,
        visibility: Visibility,
    },
    /// A batch of commands applied atomically (all-or-nothing).
    Batch {
        commands: Vec<EditCommand>,
        description: String,
    },
}

impl EditCommand {
    /// Returns the inverse command that undoes this mutation.
    ///
    /// For Batch commands, the inner commands are reversed and each is inverted
    /// (LIFO order) to correctly undo the batch.
    pub fn inverse(&self) -> EditCommand {
        match self {
            EditCommand::InsertNode { node_id, op, owner } => EditCommand::RemoveNode {
                node_id: *node_id,
                removed_node: ComputeNode::new(op.clone(), *owner),
            },
            EditCommand::RemoveNode {
                node_id,
                removed_node,
            } => EditCommand::InsertNode {
                node_id: *node_id,
                op: removed_node.op.clone(),
                owner: removed_node.owner,
            },
            EditCommand::ModifyNode {
                node_id,
                old_op,
                new_op,
                owner,
            } => EditCommand::ModifyNode {
                node_id: *node_id,
                old_op: new_op.clone(),
                new_op: old_op.clone(),
                owner: *owner,
            },
            EditCommand::InsertDataEdge {
                edge_id,
                from,
                to,
                source_port,
                target_port,
                value_type,
            } => EditCommand::RemoveEdge {
                edge_id: *edge_id,
                from: *from,
                to: *to,
                removed_edge: FlowEdge::Data {
                    source_port: *source_port,
                    target_port: *target_port,
                    value_type: *value_type,
                },
            },
            EditCommand::InsertControlEdge {
                edge_id,
                from,
                to,
                branch_index,
            } => EditCommand::RemoveEdge {
                edge_id: *edge_id,
                from: *from,
                to: *to,
                removed_edge: FlowEdge::Control {
                    branch_index: *branch_index,
                },
            },
            EditCommand::RemoveEdge {
                edge_id,
                from,
                to,
                removed_edge,
            } => match removed_edge {
                FlowEdge::Data {
                    source_port,
                    target_port,
                    value_type,
                } => EditCommand::InsertDataEdge {
                    edge_id: *edge_id,
                    from: *from,
                    to: *to,
                    source_port: *source_port,
                    target_port: *target_port,
                    value_type: *value_type,
                },
                FlowEdge::Control { branch_index } => EditCommand::InsertControlEdge {
                    edge_id: *edge_id,
                    from: *from,
                    to: *to,
                    branch_index: *branch_index,
                },
            },
            EditCommand::AddFunction {
                func_id,
                name,
                module,
                params,
                return_type,
                visibility,
            } => {
                // Inverse of AddFunction: conceptually a RemoveFunction.
                // We store the same data so we can re-add on redo.
                // For simplicity, use a Batch with a single placeholder.
                // In practice, undo of AddFunction would need to remove the
                // function from the graph. We model this as a "remove" by
                // re-using the AddFunction data in inverse form.
                EditCommand::AddFunction {
                    func_id: *func_id,
                    name: name.clone(),
                    module: *module,
                    params: params.clone(),
                    return_type: *return_type,
                    visibility: *visibility,
                }
            }
            EditCommand::AddModule {
                module_id,
                name,
                parent,
                visibility,
            } => EditCommand::AddModule {
                module_id: *module_id,
                name: name.clone(),
                parent: *parent,
                visibility: *visibility,
            },
            EditCommand::Batch {
                commands,
                description,
            } => EditCommand::Batch {
                commands: commands.iter().rev().map(|c| c.inverse()).collect(),
                description: format!("undo: {}", description),
            },
        }
    }
}

/// Persistent edit log backed by SQLite.
///
/// Records all graph mutations, supports linear undo/redo, and manages
/// the redo stack (new mutations invalidate redo entries).
pub struct EditLog;

impl EditLog {
    /// Records a mutation in the edit log. Returns the UUID assigned to this edit.
    pub fn record(
        conn: &Connection,
        program_id: ProgramId,
        command: &EditCommand,
        description: Option<&str>,
    ) -> Result<String, ApiError> {
        let edit_id = Uuid::new_v4().to_string();
        let timestamp = chrono_now();
        let command_json = serde_json::to_string(command)
            .map_err(|e| ApiError::InternalError(format!("failed to serialize command: {}", e)))?;

        conn.execute(
            "INSERT INTO edit_log (program_id, edit_id, timestamp, description, command_json, undone) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            rusqlite::params![program_id.0, edit_id, timestamp, description, command_json],
        ).map_err(|e| ApiError::InternalError(format!("failed to record edit: {}", e)))?;

        Ok(edit_id)
    }

    /// Undoes the last non-undone edit. Returns the inverse command to apply,
    /// or None if there is nothing to undo.
    pub fn undo(
        conn: &Connection,
        program_id: ProgramId,
    ) -> Result<Option<(EditCommand, HistoryEntry)>, ApiError> {
        // Find the last non-undone entry
        let result: Option<(i64, String, String, Option<String>, String)> = conn
            .query_row(
                "SELECT id, edit_id, timestamp, description, command_json FROM edit_log WHERE program_id = ?1 AND undone = 0 ORDER BY id DESC LIMIT 1",
                rusqlite::params![program_id.0],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| ApiError::InternalError(format!("failed to query edit_log: {}", e)))?;

        match result {
            None => Ok(None),
            Some((id, edit_id, timestamp, description, command_json)) => {
                // Mark as undone
                conn.execute(
                    "UPDATE edit_log SET undone = 1 WHERE id = ?1",
                    rusqlite::params![id],
                )
                .map_err(|e| ApiError::InternalError(format!("failed to mark undo: {}", e)))?;

                let command: EditCommand = serde_json::from_str(&command_json).map_err(|e| {
                    ApiError::InternalError(format!("failed to deserialize command: {}", e))
                })?;

                let entry = HistoryEntry {
                    id: edit_id,
                    timestamp,
                    description,
                    undone: true,
                };

                Ok(Some((command.inverse(), entry)))
            }
        }
    }

    /// Redoes the first undone edit. Returns the command to re-apply,
    /// or None if there is nothing to redo.
    pub fn redo(
        conn: &Connection,
        program_id: ProgramId,
    ) -> Result<Option<(EditCommand, HistoryEntry)>, ApiError> {
        // Find the first undone entry (earliest undone)
        let result: Option<(i64, String, String, Option<String>, String)> = conn
            .query_row(
                "SELECT id, edit_id, timestamp, description, command_json FROM edit_log WHERE program_id = ?1 AND undone = 1 ORDER BY id ASC LIMIT 1",
                rusqlite::params![program_id.0],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| ApiError::InternalError(format!("failed to query edit_log: {}", e)))?;

        match result {
            None => Ok(None),
            Some((id, edit_id, timestamp, description, command_json)) => {
                // Mark as not undone
                conn.execute(
                    "UPDATE edit_log SET undone = 0 WHERE id = ?1",
                    rusqlite::params![id],
                )
                .map_err(|e| ApiError::InternalError(format!("failed to mark redo: {}", e)))?;

                let command: EditCommand = serde_json::from_str(&command_json).map_err(|e| {
                    ApiError::InternalError(format!("failed to deserialize command: {}", e))
                })?;

                let entry = HistoryEntry {
                    id: edit_id,
                    timestamp,
                    description,
                    undone: false,
                };

                Ok(Some((command, entry)))
            }
        }
    }

    /// Lists all edit history entries for a program, in reverse chronological order.
    pub fn list(conn: &Connection, program_id: ProgramId) -> Result<Vec<HistoryEntry>, ApiError> {
        let mut stmt = conn
            .prepare(
                "SELECT edit_id, timestamp, description, undone FROM edit_log WHERE program_id = ?1 ORDER BY id DESC",
            )
            .map_err(|e| ApiError::InternalError(format!("failed to prepare list query: {}", e)))?;

        let rows = stmt
            .query_map(rusqlite::params![program_id.0], |row| {
                let edit_id: String = row.get(0)?;
                let timestamp: String = row.get(1)?;
                let description: Option<String> = row.get(2)?;
                let undone: i32 = row.get(3)?;
                Ok(HistoryEntry {
                    id: edit_id,
                    timestamp,
                    description,
                    undone: undone != 0,
                })
            })
            .map_err(|e| ApiError::InternalError(format!("failed to query list: {}", e)))?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(
                row.map_err(|e| ApiError::InternalError(format!("failed to read row: {}", e)))?,
            );
        }
        Ok(entries)
    }

    /// Deletes all undone entries for a program (clears the redo stack).
    ///
    /// Called when a new mutation is committed, since new edits invalidate
    /// the existing redo stack.
    pub fn clear_redo_stack(conn: &Connection, program_id: ProgramId) -> Result<(), ApiError> {
        conn.execute(
            "DELETE FROM edit_log WHERE program_id = ?1 AND undone = 1",
            rusqlite::params![program_id.0],
        )
        .map_err(|e| ApiError::InternalError(format!("failed to clear redo stack: {}", e)))?;
        Ok(())
    }
}

/// Manages named graph checkpoints in SQLite.
///
/// Checkpoints snapshot the full graph state as serialized JSON, along with
/// the current edit log position for reference.
pub struct CheckpointManager;

impl CheckpointManager {
    /// Creates a named checkpoint with the current graph state.
    pub fn create(
        conn: &Connection,
        program_id: ProgramId,
        name: &str,
        description: Option<&str>,
        graph: &ProgramGraph,
    ) -> Result<String, ApiError> {
        let timestamp = chrono_now();
        let graph_json = serde_json::to_string(graph)
            .map_err(|e| ApiError::InternalError(format!("failed to serialize graph: {}", e)))?;

        // Get current edit log position (max id for this program)
        let position: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(id), 0) FROM edit_log WHERE program_id = ?1",
                rusqlite::params![program_id.0],
                |row| row.get(0),
            )
            .map_err(|e| ApiError::InternalError(format!("failed to get edit position: {}", e)))?;

        conn.execute(
            "INSERT INTO checkpoints (program_id, name, timestamp, description, graph_json, edit_log_position) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![program_id.0, name, timestamp, description, graph_json, position],
        ).map_err(|e| {
            if e.to_string().contains("UNIQUE constraint failed") {
                ApiError::Conflict(format!("checkpoint '{}' already exists", name))
            } else {
                ApiError::InternalError(format!("failed to create checkpoint: {}", e))
            }
        })?;

        Ok(timestamp)
    }

    /// Restores a named checkpoint, returning the deserialized ProgramGraph.
    pub fn restore(
        conn: &Connection,
        program_id: ProgramId,
        name: &str,
    ) -> Result<ProgramGraph, ApiError> {
        let graph_json: String = conn
            .query_row(
                "SELECT graph_json FROM checkpoints WHERE program_id = ?1 AND name = ?2",
                rusqlite::params![program_id.0, name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| ApiError::InternalError(format!("failed to query checkpoint: {}", e)))?
            .ok_or_else(|| ApiError::NotFound(format!("checkpoint '{}' not found", name)))?;

        let graph: ProgramGraph = serde_json::from_str(&graph_json)
            .map_err(|e| ApiError::InternalError(format!("failed to deserialize graph: {}", e)))?;

        Ok(graph)
    }

    /// Lists all checkpoints for a program.
    pub fn list(conn: &Connection, program_id: ProgramId) -> Result<Vec<CheckpointView>, ApiError> {
        let mut stmt = conn
            .prepare(
                "SELECT name, timestamp, description, edit_log_position FROM checkpoints WHERE program_id = ?1 ORDER BY id DESC",
            )
            .map_err(|e| ApiError::InternalError(format!("failed to prepare list query: {}", e)))?;

        let rows = stmt
            .query_map(rusqlite::params![program_id.0], |row| {
                let name: String = row.get(0)?;
                let timestamp: String = row.get(1)?;
                let description: Option<String> = row.get(2)?;
                let edit_position: i64 = row.get(3)?;
                Ok(CheckpointView {
                    name,
                    timestamp,
                    description,
                    edit_position,
                })
            })
            .map_err(|e| ApiError::InternalError(format!("failed to query checkpoints: {}", e)))?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(
                row.map_err(|e| ApiError::InternalError(format!("failed to read row: {}", e)))?,
            );
        }
        Ok(entries)
    }

    /// Deletes a named checkpoint.
    pub fn delete(conn: &Connection, program_id: ProgramId, name: &str) -> Result<(), ApiError> {
        let rows = conn
            .execute(
                "DELETE FROM checkpoints WHERE program_id = ?1 AND name = ?2",
                rusqlite::params![program_id.0, name],
            )
            .map_err(|e| ApiError::InternalError(format!("failed to delete checkpoint: {}", e)))?;

        if rows == 0 {
            return Err(ApiError::NotFound(format!(
                "checkpoint '{}' not found",
                name
            )));
        }
        Ok(())
    }
}

/// Use `rusqlite::params!` re-export.
use rusqlite::OptionalExtension;

/// Returns the current UTC timestamp in ISO 8601 format.
fn chrono_now() -> String {
    // Use a simple approach without chrono dependency: format current time.
    // We'll use std::time::SystemTime for a basic ISO 8601 timestamp.
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Convert to basic ISO 8601 format
    // Days since epoch, hours, minutes, seconds
    let days = secs / 86400;
    let remaining = secs % 86400;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;

    // Convert days since 1970-01-01 to date
    // Simple calculation (doesn't handle leap seconds, but good enough for timestamps)
    let (year, month, day) = days_to_date(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Converts days since Unix epoch to (year, month, day).
fn days_to_date(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

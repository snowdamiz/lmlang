-- Edit history and checkpoint tables for undo/redo system.
-- Tracks all graph mutations with serialized command data, and supports
-- named checkpoints that snapshot the full graph state.

CREATE TABLE IF NOT EXISTS edit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    program_id INTEGER NOT NULL REFERENCES programs(id),
    edit_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    description TEXT,
    command_json TEXT NOT NULL,
    undone INTEGER NOT NULL DEFAULT 0,
    UNIQUE(program_id, edit_id)
);

CREATE INDEX idx_edit_log_program ON edit_log(program_id, id);

CREATE TABLE IF NOT EXISTS checkpoints (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    program_id INTEGER NOT NULL REFERENCES programs(id),
    name TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    description TEXT,
    graph_json TEXT NOT NULL,
    edit_log_position INTEGER NOT NULL,
    UNIQUE(program_id, name)
);

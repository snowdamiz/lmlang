-- Migration 1: Initial schema for lmlang program storage.
--
-- Normalized relational schema: separate tables for programs, types, modules,
-- functions, compute nodes, flow edges, semantic nodes, and semantic edges.
-- JSON TEXT columns store complex Rust enum types (ComputeNodeOp, FlowEdge, etc.)
-- via serde_json serialization.

CREATE TABLE programs (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE types (
    program_id  INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    type_id     INTEGER NOT NULL,
    type_json   TEXT NOT NULL,
    name        TEXT,
    PRIMARY KEY (program_id, type_id)
);

CREATE TABLE modules (
    program_id  INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    module_id   INTEGER NOT NULL,
    name        TEXT NOT NULL,
    parent_id   INTEGER,
    visibility  TEXT NOT NULL,
    PRIMARY KEY (program_id, module_id),
    FOREIGN KEY (program_id, parent_id) REFERENCES modules(program_id, module_id)
);

CREATE TABLE functions (
    program_id      INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    function_id     INTEGER NOT NULL,
    name            TEXT NOT NULL,
    module_id       INTEGER NOT NULL,
    visibility      TEXT NOT NULL,
    params_json     TEXT NOT NULL,
    return_type_id  INTEGER NOT NULL,
    entry_node_id   INTEGER,
    is_closure      INTEGER NOT NULL DEFAULT 0,
    parent_function INTEGER,
    captures_json   TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (program_id, function_id),
    FOREIGN KEY (program_id, module_id) REFERENCES modules(program_id, module_id)
);

CREATE TABLE compute_nodes (
    program_id   INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    node_id      INTEGER NOT NULL,
    owner_fn_id  INTEGER NOT NULL,
    op_json      TEXT NOT NULL,
    PRIMARY KEY (program_id, node_id),
    FOREIGN KEY (program_id, owner_fn_id) REFERENCES functions(program_id, function_id)
);

CREATE TABLE flow_edges (
    program_id   INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    edge_id      INTEGER NOT NULL,
    source_id    INTEGER NOT NULL,
    target_id    INTEGER NOT NULL,
    edge_json    TEXT NOT NULL,
    PRIMARY KEY (program_id, edge_id),
    FOREIGN KEY (program_id, source_id) REFERENCES compute_nodes(program_id, node_id),
    FOREIGN KEY (program_id, target_id) REFERENCES compute_nodes(program_id, node_id)
);

CREATE TABLE semantic_nodes (
    program_id   INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    node_idx     INTEGER NOT NULL,
    node_json    TEXT NOT NULL,
    PRIMARY KEY (program_id, node_idx)
);

CREATE TABLE semantic_edges (
    program_id   INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    edge_idx     INTEGER NOT NULL,
    source_idx   INTEGER NOT NULL,
    target_idx   INTEGER NOT NULL,
    edge_type    TEXT NOT NULL,
    PRIMARY KEY (program_id, edge_idx),
    FOREIGN KEY (program_id, source_idx) REFERENCES semantic_nodes(program_id, node_idx),
    FOREIGN KEY (program_id, target_idx) REFERENCES semantic_nodes(program_id, node_idx)
);

-- Indices for common queries
CREATE INDEX idx_compute_nodes_owner ON compute_nodes(program_id, owner_fn_id);
CREATE INDEX idx_flow_edges_source ON flow_edges(program_id, source_id);
CREATE INDEX idx_flow_edges_target ON flow_edges(program_id, target_id);
CREATE INDEX idx_functions_module ON functions(program_id, module_id);
CREATE INDEX idx_modules_parent ON modules(program_id, parent_id);

# Phase 4: AI Agent Tool API - Research

**Researched:** 2026-02-18
**Domain:** HTTP/JSON API wrapping graph mutations, queries, verification, simulation, and undo
**Confidence:** HIGH

## Summary

Phase 4 builds an HTTP/JSON API layer on top of the existing `lmlang-core` (graph model), `lmlang-storage` (SQLite persistence), and `lmlang-check` (type checking + interpreter) crates. The API exposes five core capabilities: structured graph mutations with validation (TOOL-01), focused subgraph retrieval (TOOL-02), type-checking with diagnostics (TOOL-03), subgraph simulation via the interpreter (TOOL-04), and undo/rollback via edit history (STORE-03). All exposed as HTTP/JSON endpoints via axum (TOOL-05) with structured error diagnostics (TOOL-06).

The existing codebase provides a solid foundation. All core types (`NodeId`, `EdgeId`, `FunctionId`, `ModuleId`, `ComputeNodeOp`, `FlowEdge`, etc.) already derive `Serialize`/`Deserialize`. The `ProgramGraph` has mutation methods (`add_compute_node`, `add_data_edge`, `remove_compute_node`, `remove_edge`, `add_function`, `add_module`). The `GraphStore` trait provides full CRUD. The type checker provides `validate_data_edge` (per-edit) and `validate_graph` (full scan). The interpreter supports `start`/`run`/`step`/`pause`/`resume` with tracing and partial results on error. The main work is: (1) designing the API request/response schema, (2) building the undo/history system, (3) wiring everything through axum handlers, and (4) adding query capabilities not yet in the core (N-hop neighborhood, search/filter).

**Primary recommendation:** Create a new `lmlang-server` crate using axum 0.8. Structure as thin HTTP handlers delegating to a `ProgramService` application layer that coordinates graph, storage, checker, and interpreter. Implement undo via command-pattern with SQLite-backed edit log.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **Mutation workflow**: Support both single-operation and batch mutation modes. `dry_run` flag on each call: dry_run=true previews validation results, dry_run=false validates and commits atomically. Batch mutations are all-or-nothing.
- **Query & context shape**: No default query scope -- every query must explicitly specify what it wants. Both program overview and search/filter endpoints. Agent-controlled response verbosity via detail parameter (summary/standard/full). Query responses can include derived/computed information when requested.
- **Error & diagnostics**: Layered diagnostic format: top-level summary (error code + message) with optional details field. Errors describe the problem only -- no fix suggestions. Two severity levels: errors (block commit) and warnings (informational). Verification scope is agent-controlled: 'local' or 'full'.
- **Undo & versioning**: Both linear undo stack and named checkpoints. Undo history is persistent (survives across sessions, stored in database). Unlimited history depth. History is inspectable: list past mutations, view checkpoint metadata, diff between versions.

### Claude's Discretion
- Whether to include high-level compound operations (add_function, replace_subgraph) alongside primitives, or start with primitives only
- HTTP framework and routing design
- Request/response serialization details
- Internal architecture for the undo storage

### Deferred Ideas (OUT OF SCOPE)
None
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| TOOL-01 | `propose_structured_edit` accepts structured graph mutations with validation before commit | Existing `ProgramGraph` mutation methods + `validate_data_edge`/`validate_graph` from `lmlang-check`. Need API schema wrapping these + dry_run/batch semantics. |
| TOOL-02 | `retrieve_subgraph` returns focused graph context by node ID, function boundary, N-hop neighborhood, or type/relationship filter | Existing `function_nodes`, `find_nodes_by_owner`, `find_edges_from/to` in storage. Need N-hop BFS traversal and detail-level serialization. |
| TOOL-03 | `verify_and_propagate` type-checks affected subgraph, runs contract checks, marks dirty nodes | Existing `validate_graph` and `validate_data_edge` in `lmlang-check`. Dirty node tracking via `hash_function`/`hash_all_functions` in storage. Contract checks are Phase 5 (stub here). |
| TOOL-04 | `simulate` executes subgraph with provided inputs, returns output values and execution trace | Existing `Interpreter` with `start`/`run`, `TraceEntry`, and `ExecutionState::Completed/Error` with partial results. Need serde for `Value` and `TraceEntry`. |
| TOOL-05 | Tool API exposed as HTTP/JSON endpoints via axum | axum 0.8 with `Json` extractors, `State` for shared app state, tower-http for CORS/tracing. |
| TOOL-06 | Error responses include structured diagnostics with graph location context | Existing `TypeError` variants already contain node IDs, ports, expected/actual types, function IDs. Need serde serialization. |
| STORE-03 | User can undo/rollback edits via edit log or graph snapshots | New undo system: command-pattern edit log in SQLite + named checkpoints via full graph snapshots. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| axum | 0.8 | HTTP routing, JSON extraction, state management | Dominant Rust web framework, tokio-native, type-safe extractors, macro-free |
| tokio | 1.x (features=["full"]) | Async runtime | Required by axum, the standard async runtime for Rust |
| tower | 0.5 | Middleware composition | axum's middleware system, enables layered services |
| tower-http | 0.6 (features=["cors","trace"]) | CORS, request tracing middleware | Standard companion for axum, provides production-ready middleware |
| serde | 1.0 (features=["derive"]) | Serialization/deserialization | Already used throughout codebase |
| serde_json | 1.0 | JSON processing | Already used throughout codebase |
| thiserror | 2.0 | Error type definitions | Already used throughout codebase |
| uuid | 1 (features=["v4","serde"]) | Edit operation and checkpoint IDs | Standard for unique identifiers in APIs |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tracing | 0.1 | Structured logging | Request logging, error diagnostics |
| tracing-subscriber | 0.3 | Log output formatting | Console output for development |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| axum | actix-web | actix-web has slightly higher raw throughput but axum's tower integration and type safety are better for this use case |
| command-pattern undo | full graph snapshots only | Snapshots are simpler but use more storage; command pattern gives fine-grained history |
| uuid for edit IDs | sequential i64 | uuid is globally unique and unguessable; i64 is simpler but less robust for distributed scenarios |

**Installation:**
```bash
cargo add axum@0.8 --package lmlang-server
cargo add tokio --package lmlang-server --features full
cargo add tower --package lmlang-server
cargo add tower-http --package lmlang-server --features cors,trace
cargo add serde --package lmlang-server --features derive
cargo add serde_json --package lmlang-server
cargo add thiserror@2 --package lmlang-server
cargo add uuid --package lmlang-server --features v4,serde
cargo add tracing --package lmlang-server
cargo add tracing-subscriber --package lmlang-server
```

## Architecture Patterns

### Recommended Project Structure
```
crates/lmlang-server/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports, router construction
    ├── main.rs             # Binary entrypoint (start server)
    ├── state.rs            # AppState (Arc<Mutex<ProgramService>>)
    ├── error.rs            # API error types, IntoResponse impl
    ├── service.rs          # ProgramService: coordinates graph+storage+checker
    ├── undo.rs             # EditLog, Checkpoint, undo/redo engine
    ├── handlers/
    │   ├── mod.rs
    │   ├── mutations.rs    # TOOL-01: propose_structured_edit
    │   ├── queries.rs      # TOOL-02: retrieve_subgraph, program overview
    │   ├── verify.rs       # TOOL-03: verify_and_propagate
    │   ├── simulate.rs     # TOOL-04: simulate subgraph
    │   ├── history.rs      # STORE-03: undo/redo/checkpoint endpoints
    │   └── programs.rs     # Program CRUD (create, list, delete)
    └── schema/
        ├── mod.rs
        ├── mutations.rs    # Request/response types for mutations
        ├── queries.rs      # Request/response types for queries
        ├── diagnostics.rs  # Structured error/warning response types
        ├── simulate.rs     # Simulation request/response types
        └── history.rs      # Undo/checkpoint request/response types
```

### Pattern 1: Thin Handler + Service Layer
**What:** HTTP handlers only parse requests and serialize responses. All business logic lives in `ProgramService`.
**When to use:** Always -- keeps handlers testable and framework-independent.
**Example:**
```rust
// handlers/mutations.rs
async fn propose_edit(
    State(state): State<AppState>,
    Json(request): Json<ProposeEditRequest>,
) -> Result<Json<ProposeEditResponse>, ApiError> {
    let mut service = state.service.lock().unwrap();
    let result = service.propose_edit(request)?;
    Ok(Json(result))
}
```

### Pattern 2: AppState with Arc<Mutex<ProgramService>>
**What:** Single `ProgramService` holds the in-memory `ProgramGraph` and the `SqliteStore`. Protected by `Mutex` since graph mutations are not async.
**When to use:** For this single-agent phase (Phase 7 adds multi-agent concurrency).
**Why std::sync::Mutex:** Graph operations are CPU-bound and synchronous. No `.await` points while holding the lock. `std::sync::Mutex` is simpler and faster than `tokio::sync::Mutex` when the critical section is short and synchronous.
**Example:**
```rust
#[derive(Clone)]
struct AppState {
    service: Arc<Mutex<ProgramService>>,
}

struct ProgramService {
    graph: ProgramGraph,
    store: SqliteStore,
    program_id: ProgramId,
    edit_log: EditLog,
}
```

### Pattern 3: Layered Diagnostic Response
**What:** All API responses use a consistent envelope format with error code, message, optional details, and optional warnings.
**When to use:** Every response, not just errors. Success responses include warnings too.
**Example:**
```rust
#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ApiErrorDetail>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<DiagnosticWarning>,
}

#[derive(Serialize)]
struct ApiErrorDetail {
    code: String,              // e.g., "TYPE_MISMATCH", "NODE_NOT_FOUND"
    message: String,           // Human-readable summary
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,  // Full context (failing nodes, types, etc.)
}
```

### Pattern 4: Command-Pattern Edit Log for Undo
**What:** Each mutation is recorded as a reversible `EditCommand` in the edit log. Undo pops and reverses. Checkpoints snapshot the full graph state.
**When to use:** Every committed mutation (not dry_run).
**Example:**
```rust
#[derive(Serialize, Deserialize)]
enum EditCommand {
    InsertNode { node_id: NodeId, node: ComputeNode },
    RemoveNode { node_id: NodeId, removed: ComputeNode },
    InsertEdge { edge_id: EdgeId, from: NodeId, to: NodeId, edge: FlowEdge },
    RemoveEdge { edge_id: EdgeId, from: NodeId, to: NodeId, removed: FlowEdge },
    ModifyNode { node_id: NodeId, old: ComputeNode, new: ComputeNode },
    AddFunction { func_id: FunctionId, func_def: FunctionDef },
    // ... etc.
    Batch { commands: Vec<EditCommand>, description: String },
}
```

### Pattern 5: Detail-Level Response Control
**What:** Queries accept a `detail` parameter (summary/standard/full) that controls how much information is returned per node/edge.
**When to use:** All query responses.
**Example:**
```rust
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum DetailLevel {
    Summary,   // Node ID + op name only
    Standard,  // + owner function, edge connections
    Full,      // + all op parameters, port types, complete edge data
}
```

### Anti-Patterns to Avoid
- **Mixing framework logic with business logic:** Handlers should not directly manipulate ProgramGraph -- always go through ProgramService.
- **Holding Mutex across async boundaries:** Never `.await` while holding the service lock. Lock, do synchronous work, unlock, then respond.
- **Mutable global graph without undo recording:** Every mutation to the graph MUST go through the edit log. Direct ProgramGraph mutations bypass history.
- **Opaque error strings:** Never return plain text errors. Always use the structured `ApiErrorDetail` with error codes and node/edge context.
- **Leaking internal petgraph indices:** API uses `NodeId(u32)` and `EdgeId(u32)` -- never expose raw `NodeIndex<u32>` or `EdgeIndex<u32>`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| HTTP routing | Custom TCP/HTTP parsing | axum 0.8 `Router` | Battle-tested, type-safe, zero boilerplate |
| JSON parsing/serialization | Manual JSON construction | serde + axum `Json<T>` | Compile-time safety, automatic validation |
| CORS handling | Manual header injection | tower-http `CorsLayer` | Handles preflight requests, complex CORS rules |
| Request tracing | Manual timing/logging | tower-http `TraceLayer` + tracing | Structured, async-aware, configurable |
| Unique IDs for edits | Custom ID generation | uuid v4 | Cryptographically random, no collision risk |
| Graph traversal for N-hop | Custom recursive walk | petgraph BFS/DFS visitors | Optimized, handles cycles, well-tested |

**Key insight:** The existing codebase already handles the hard problems (graph consistency, type checking, interpretation). Phase 4 is primarily a translation layer -- translating HTTP requests into calls to existing Rust APIs and formatting the results as JSON. Don't re-implement what's already there.

## Common Pitfalls

### Pitfall 1: Graph Consistency During Batch Mutations
**What goes wrong:** Applying mutations one-by-one in a batch leaves the graph in an inconsistent intermediate state if a later mutation fails.
**Why it happens:** Individual mutations may create nodes before their edges, or edges before their target nodes.
**How to avoid:** Clone the graph before batch execution. Apply all mutations to the clone. If all succeed validation, swap the clone with the original. If any fail, discard the clone and return the error.
**Warning signs:** Partial mutations visible in query results after a batch failure.

### Pitfall 2: Undo Command Ordering
**What goes wrong:** Undoing a batch in the wrong order (e.g., removing a node before its edges) causes graph integrity errors.
**Why it happens:** Commands must be reversed in LIFO order, and edge removal must precede node removal.
**How to avoid:** Store batch commands in order. On undo, reverse the entire list and apply inverse operations. Test with round-trip: apply -> undo -> verify graph matches original.
**Warning signs:** "node not found" or "edge not found" errors during undo operations.

### Pitfall 3: Serde Compatibility Between Layers
**What goes wrong:** The API returns JSON that doesn't match what AI agents expect because internal Rust enum serialization uses serde's default tagged format.
**Why it happens:** `ComputeOp::BinaryArith { op: ArithOp::Add }` serializes as `{"BinaryArith":{"op":"Add"}}` which may surprise agents expecting flat structures.
**How to avoid:** Define explicit API schema types in `schema/` that translate from internal types. Don't expose internal serde representations directly in the API. This also decouples the API contract from internal refactoring.
**Warning signs:** Agent requests failing to deserialize, or response format changing when internal types change.

### Pitfall 4: Blocking the Tokio Runtime
**What goes wrong:** Synchronous SQLite operations (rusqlite is blocking) starve the async runtime.
**Why it happens:** `rusqlite` operations are synchronous and can block the tokio worker thread.
**How to avoid:** Use `tokio::task::spawn_blocking` for any operation that touches SQLite, or accept the blocking since this is single-agent (Phase 7 will need to address this). For Phase 4, keeping it simple with `std::sync::Mutex` and short critical sections is acceptable.
**Warning signs:** High latency on concurrent requests, timeout errors.

### Pitfall 5: N-Hop Neighborhood Explosion
**What goes wrong:** Requesting a 5-hop neighborhood from a central node returns the entire graph.
**Why it happens:** Small-world property of graphs -- hop count grows exponentially.
**How to avoid:** Cap maximum hop count (recommend max 3). Include node count in response metadata so the agent can adjust. Consider limiting results to a maximum node count.
**Warning signs:** Response payloads in the megabytes, slow response times.

### Pitfall 6: Missing Serde Derives on Interpreter Types
**What goes wrong:** `Value`, `TraceEntry`, `RuntimeError`, and `TypeError` can't be serialized for API responses.
**Why it happens:** These types currently derive `Debug` and `Clone` but NOT `Serialize`/`Deserialize`.
**How to avoid:** Add `#[derive(Serialize, Deserialize)]` to `Value`, `TraceEntry`, `RuntimeError` in lmlang-check. Alternatively, define API-specific response types that map from these internal types.
**Warning signs:** Compile errors when trying to return `Json<SimulateResponse>` with internal types.

## Code Examples

### Axum Router Setup with State
```rust
// Source: axum 0.8 official patterns
use axum::{Router, routing::{get, post}, extract::State};
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

fn build_router(state: AppState) -> Router {
    Router::new()
        // Program management
        .route("/programs", get(handlers::programs::list))
        .route("/programs", post(handlers::programs::create))
        // Mutations (TOOL-01)
        .route("/programs/{id}/mutations", post(handlers::mutations::propose_edit))
        .route("/programs/{id}/mutations/batch", post(handlers::mutations::batch_edit))
        // Queries (TOOL-02)
        .route("/programs/{id}/overview", get(handlers::queries::program_overview))
        .route("/programs/{id}/nodes/{node_id}", get(handlers::queries::get_node))
        .route("/programs/{id}/functions/{func_id}", get(handlers::queries::get_function))
        .route("/programs/{id}/neighborhood", post(handlers::queries::neighborhood))
        .route("/programs/{id}/search", post(handlers::queries::search))
        // Verify (TOOL-03)
        .route("/programs/{id}/verify", post(handlers::verify::verify))
        // Simulate (TOOL-04)
        .route("/programs/{id}/simulate", post(handlers::simulate::simulate))
        // History (STORE-03)
        .route("/programs/{id}/history", get(handlers::history::list_history))
        .route("/programs/{id}/undo", post(handlers::history::undo))
        .route("/programs/{id}/redo", post(handlers::history::redo))
        .route("/programs/{id}/checkpoints", post(handlers::history::create_checkpoint))
        .route("/programs/{id}/checkpoints", get(handlers::history::list_checkpoints))
        .route("/programs/{id}/checkpoints/{name}/restore", post(handlers::history::restore_checkpoint))
        .route("/programs/{id}/diff", post(handlers::history::diff_versions))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
```

### Mutation Request Schema
```rust
// Source: derived from CONTEXT.md decisions + existing ProgramGraph API

#[derive(Deserialize)]
struct ProposeEditRequest {
    /// What to mutate
    mutations: Vec<Mutation>,
    /// If true, validate only -- don't commit
    dry_run: bool,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Mutation {
    #[serde(rename = "insert_node")]
    InsertNode {
        op: ComputeNodeOp,
        owner: FunctionId,
    },
    #[serde(rename = "remove_node")]
    RemoveNode {
        node_id: NodeId,
    },
    #[serde(rename = "modify_node")]
    ModifyNode {
        node_id: NodeId,
        new_op: ComputeNodeOp,
    },
    #[serde(rename = "add_edge")]
    AddEdge {
        from: NodeId,
        to: NodeId,
        source_port: u16,
        target_port: u16,
        value_type: TypeId,
    },
    #[serde(rename = "add_control_edge")]
    AddControlEdge {
        from: NodeId,
        to: NodeId,
        branch_index: Option<u16>,
    },
    #[serde(rename = "remove_edge")]
    RemoveEdge {
        edge_id: EdgeId,
    },
    #[serde(rename = "add_function")]
    AddFunction {
        name: String,
        module: ModuleId,
        params: Vec<(String, TypeId)>,
        return_type: TypeId,
        visibility: Visibility,
    },
    #[serde(rename = "add_module")]
    AddModule {
        name: String,
        parent: ModuleId,
        visibility: Visibility,
    },
}

#[derive(Serialize)]
struct ProposeEditResponse {
    /// Whether all mutations were valid
    valid: bool,
    /// IDs assigned to newly created entities (in order of mutations)
    created: Vec<CreatedEntity>,
    /// Validation errors (block commit)
    errors: Vec<DiagnosticError>,
    /// Validation warnings (informational)
    warnings: Vec<DiagnosticWarning>,
    /// Whether changes were committed (false if dry_run=true or errors exist)
    committed: bool,
}
```

### Query Response with Detail Levels
```rust
// Source: derived from CONTEXT.md detail parameter decision

#[derive(Serialize)]
struct NodeView {
    id: NodeId,
    op: String,           // Always: human-readable op name
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<FunctionId>,    // standard+
    #[serde(skip_serializing_if = "Option::is_none")]
    op_data: Option<serde_json::Value>,  // full: complete op enum
    #[serde(skip_serializing_if = "Option::is_none")]
    incoming_edges: Option<Vec<EdgeView>>,  // standard+
    #[serde(skip_serializing_if = "Option::is_none")]
    outgoing_edges: Option<Vec<EdgeView>>,  // standard+
}

#[derive(Serialize)]
struct EdgeView {
    id: EdgeId,
    from: NodeId,
    to: NodeId,
    kind: String,         // "data" or "control"
    #[serde(skip_serializing_if = "Option::is_none")]
    value_type: Option<TypeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch_index: Option<u16>,
}
```

### Undo System: SQLite Edit Log Schema
```sql
-- Source: custom design for STORE-03

-- Edit operations log
CREATE TABLE IF NOT EXISTS edit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    program_id INTEGER NOT NULL REFERENCES programs(id),
    edit_id TEXT NOT NULL,           -- UUID v4
    timestamp TEXT NOT NULL,          -- ISO 8601
    description TEXT,                 -- Human-readable description
    command_json TEXT NOT NULL,        -- Serialized EditCommand
    undone INTEGER NOT NULL DEFAULT 0, -- 0=active, 1=undone
    UNIQUE(program_id, edit_id)
);

-- Named checkpoints (full graph snapshots)
CREATE TABLE IF NOT EXISTS checkpoints (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    program_id INTEGER NOT NULL REFERENCES programs(id),
    name TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    description TEXT,
    graph_json TEXT NOT NULL,          -- Full ProgramGraph serialized
    edit_log_position INTEGER NOT NULL, -- edit_log.id at checkpoint time
    UNIQUE(program_id, name)
);
```

### Structured Diagnostic for TOOL-06
```rust
// Source: derived from existing TypeError + CONTEXT.md decisions

#[derive(Serialize)]
struct DiagnosticError {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<DiagnosticDetails>,
}

#[derive(Serialize)]
struct DiagnosticDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    source_node: Option<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_node: Option<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    edge_path: Option<Vec<EdgeId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_type: Option<TypeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actual_type: Option<TypeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    function_id: Option<FunctionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
}

// Conversion from existing TypeError:
impl From<TypeError> for DiagnosticError {
    fn from(err: TypeError) -> Self {
        match err {
            TypeError::TypeMismatch {
                source_node, target_node, source_port, target_port,
                expected, actual, function_id, ..
            } => DiagnosticError {
                code: "TYPE_MISMATCH".into(),
                message: format!("port {} expects {:?}, got {:?}", target_port, expected, actual),
                details: Some(DiagnosticDetails {
                    source_node: Some(source_node),
                    target_node: Some(target_node),
                    expected_type: Some(expected),
                    actual_type: Some(actual),
                    function_id: Some(function_id),
                    port: Some(target_port),
                    edge_path: None,
                }),
            },
            // ... other variants
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| axum 0.7 `/:param` path syntax | axum 0.8 `/{param}` path syntax | Jan 2025 | Must use new syntax in all route definitions |
| Manual async middleware | tower-http layered middleware | Stable | Use `ServiceBuilder` for middleware composition |
| `Option<T>` silently ignoring extractors | `OptionalFromRequestParts` trait | axum 0.8 | More explicit optional extraction |

**Deprecated/outdated:**
- axum `/:param` and `/*wildcard` path syntax (use `/{param}` and `/{*wildcard}` in 0.8)
- `axum::Extension` for state (use `axum::extract::State` with `with_state`)

## Discretion Recommendations

### Compound Operations: Include Them
**Recommendation:** Include high-level compound operations (`add_function`, `add_module`) alongside primitives from the start.

**Rationale:** The existing `ProgramGraph` already provides these as methods (`add_function`, `add_module`, `add_closure`). They enforce dual-graph consistency (auto-creating semantic nodes). Forcing agents to manually create semantic nodes would be error-prone and tedious. The compound operations map directly to existing API surface -- zero extra implementation cost.

Primitives to expose: `insert_node`, `remove_node`, `modify_node`, `add_edge`, `add_control_edge`, `remove_edge`.
Compound operations to expose: `add_function`, `add_module`, `add_closure`. (Subgraph replace can be a batch of primitives.)

### HTTP Framework: axum 0.8
**Recommendation:** Use axum 0.8 as specified in TOOL-05.

**Rationale:** Already the community standard for Rust HTTP APIs. The `Json<T>` extractor pattern maps perfectly to this API's needs. `State<AppState>` handles shared state cleanly. tower-http provides production middleware with zero custom code.

### Undo Storage Architecture
**Recommendation:** Hybrid command-pattern + checkpoint snapshots stored in SQLite.

**Rationale:** The command pattern (edit log with inverse operations) gives fine-grained undo history with low storage cost per edit. Named checkpoints use full graph JSON snapshots for fast restore without replaying history. SQLite storage ensures persistence across sessions (locked decision). The edit log also enables the "inspectable history" requirement (list past mutations, diff between versions).

The edit log table and checkpoints table both go into the existing SQLite database used by `lmlang-storage`, added via a new migration (002_edit_history.sql).

## Open Questions

1. **Multi-program support in a single server instance**
   - What we know: `GraphStore` is per-program (`ProgramId`). The server could manage multiple programs.
   - What's unclear: Should the server start with one program loaded, or support dynamic program switching?
   - Recommendation: Support multiple programs via URL path parameter (`/programs/{id}/...`). The `ProgramService` loads the active program on demand. Start simple: keep one `ProgramGraph` in memory at a time, load/unload on program switch.

2. **Verification scope for 'local' mode**
   - What we know: CONTEXT.md says agent chooses 'local' (affected subgraph + immediate dependents) or 'full' (entire program).
   - What's unclear: How to efficiently identify "affected subgraph + immediate dependents" after a mutation.
   - Recommendation: Track affected node IDs during mutation, then validate only edges touching those nodes. Use `find_edges_from`/`find_edges_to` to find immediate dependents. This is a subset of `validate_graph` that only visits relevant nodes.

3. **Graph diff for version comparison**
   - What we know: History must be inspectable including diffs between versions.
   - What's unclear: What format the diff should take (list of added/removed/modified nodes? visual? structural?).
   - Recommendation: Structural diff -- list of added, removed, and modified nodes/edges with their data. Use the content hashing already in `lmlang-storage` (`hash_function`, `hash_all_functions`) to detect which functions changed.

## Sources

### Primary (HIGH confidence)
- Existing codebase: `lmlang-core/src/graph.rs` -- ProgramGraph API surface (all mutation/query methods)
- Existing codebase: `lmlang-storage/src/traits.rs` -- GraphStore trait (full CRUD + query methods)
- Existing codebase: `lmlang-check/src/typecheck/mod.rs` -- validate_data_edge and validate_graph
- Existing codebase: `lmlang-check/src/interpreter/` -- Interpreter, Value, TraceEntry, RuntimeError, ExecutionState
- Existing codebase: `lmlang-core/src/id.rs` -- All IDs already derive Serialize/Deserialize
- [axum official docs](https://docs.rs/axum/latest/axum/) -- Router, Json extractor, State, handler patterns
- [axum 0.8.0 announcement](https://tokio.rs/blog/2025-01-01-announcing-axum-0-8-0) -- New path syntax, breaking changes

### Secondary (MEDIUM confidence)
- [tower-http CORS docs](https://docs.rs/tower-http/latest/tower_http/cors/index.html) -- CorsLayer configuration
- [axum State docs](https://docs.rs/axum/latest/axum/extract/struct.State.html) -- Arc<Mutex<T>> pattern for shared mutable state
- [tokio shared state guide](https://tokio.rs/tokio/tutorial/shared-state) -- std::sync::Mutex vs tokio::sync::Mutex guidance

### Tertiary (LOW confidence)
- [undo crate](https://github.com/evenorog/undo) -- Reviewed but not adopting; building custom command pattern is simpler for this specific use case (we need SQLite persistence and graph-specific commands)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- axum 0.8 is well-documented, existing codebase types are serde-ready
- Architecture: HIGH -- thin handler + service layer is standard pattern, well-understood
- Mutations/queries: HIGH -- maps directly to existing ProgramGraph and GraphStore APIs
- Undo system: MEDIUM -- command pattern is well-understood but implementation details (batch undo ordering, checkpoint storage size) need validation during implementation
- Pitfalls: HIGH -- based on direct codebase analysis (e.g., missing Serialize derives on Value/TraceEntry)

**Research date:** 2026-02-18
**Valid until:** 2026-03-18 (30 days -- stack is stable)

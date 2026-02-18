# Phase 2: Storage & Persistence - Research

**Researched:** 2026-02-18
**Domain:** Rust SQLite persistence, trait-based storage abstraction, content-addressable hashing, schema design for graph databases
**Confidence:** HIGH

## Summary

Phase 2 adds persistence to the lmlang graph data model built in Phase 1. The core challenge is mapping petgraph's `StableGraph<ComputeNode, FlowEdge>` and `StableGraph<SemanticNode, SemanticEdge>`, along with the TypeRegistry, ModuleTree, and FunctionDef lookup tables, into a normalized relational SQLite schema -- and then reconstructing them faithfully on load. The second challenge is designing a `GraphStore` trait that abstracts over SQLite and in-memory backends identically. The third is implementing Merkle-tree-style content hashing that composes from leaf nodes upward through edges to per-function root hashes.

The research confirms that `rusqlite` 0.38.0 (with bundled SQLite 3.51.1) is the standard Rust SQLite library, providing synchronous ergonomic bindings with transaction support, prepared statement caching, and WAL mode for concurrent reads. For schema migrations, `rusqlite_migration` is the best fit -- lightweight, no macros, uses SQLite's `user_version` pragma instead of migration tables. For content hashing, `blake3` 1.8.3 is the clear choice (fast, Merkle-tree internally, deterministic) combined with a custom canonical serialization approach that avoids HashMap iteration order issues. The existing codebase already uses `serde` with `Serialize`/`Deserialize` derives on all types, which provides a path for serializing complex enum variants (like `ComputeOp`, `LmType`, `ConstValue`) to JSON TEXT columns in SQLite.

**Primary recommendation:** Build a synchronous `GraphStore` trait with CRUD + query methods, implement the in-memory backend first (HashMap-based, mirroring SQLite table structure), then the SQLite backend using rusqlite with WAL mode and transaction batching. Use JSON TEXT columns for complex enum types (ops, edge data, type definitions) to keep the schema queryable and debuggable. Compute content hashes by canonical byte serialization of node content + sorted edge target hashes, using blake3. Hashes are derived on load and after mutations -- never stored in the database.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**Storage model:**
- Normalized relational schema: separate tables for nodes, edges, types, modules, functions
- Types in their own table with IDs; nodes and edges reference type IDs (enables type-based queries and deduplication)
- Multi-program database: single SQLite file holds multiple programs with a programs table
- Modules and functions as separate tables with parent-child hierarchy (enables querying by module path, listing functions in a module)

**Content hashing:**
- Merkle-tree style: node hashes compose upward including edge targets' hashes (structural integrity like git)
- Per-function root hashes: each function has a root hash; cross-function changes detected by comparing function roots
- Eager recomputation: hashes updated immediately on every mutation (always consistent, cost at write time)
- Hashes NOT stored in database: computed from stored content on load (DB is source of truth, hashes are derived)

**Save/load granularity:**
- Whole-program loading: load all nodes, edges, types, functions, modules in one operation
- Incremental saves: track dirty nodes/edges since last save, write only changed data
- Auto-persist: every mutation writes through to SQLite immediately (always durable)

**Trait API surface:**
- Two-layer API: low-level CRUD (insert_node, get_node, delete_node, insert_edge, etc.) as the trait foundation, with high-level convenience methods (save_program, load_program, save_function) built on top
- In-memory backend is first-class: production-quality, used for tests, ephemeral agent sessions, and anywhere persistence isn't needed
- Query methods included in trait: find_by_type, find_functions, search edges -- backend-specific optimization (SQL WHERE vs in-memory filter)

### Claude's Discretion

- Sync vs async trait design (based on Rust async ergonomics and project backend evolution)
- SQLite transaction batching strategy for grouped mutations
- Exact SQL schema design and migration approach
- Serialization format for complex types (e.g., ConstValue variants)
- Error type design for storage operations

### Deferred Ideas (OUT OF SCOPE)

None -- discussion stayed within phase scope

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| STORE-01 | Programs persist in SQLite with atomic writes and schema migration support | rusqlite 0.38.0 provides transactions for atomic writes; rusqlite_migration provides schema migration via user_version pragma; WAL mode + synchronous=NORMAL for durability with performance |
| STORE-02 | Graph storage uses a trait abstraction (GraphStore) swappable to alternative backends without core changes | Sync trait design recommended (avoids async complexity for embedded SQLite); two-layer API pattern (low-level CRUD trait + high-level convenience methods); in-memory backend as first-class implementation |
| STORE-04 | Each graph node has a deterministic content hash for identity and change detection | blake3 for fast hashing; canonical serialization of node content avoids HashMap iteration order issues (all Phase 1 types use IndexMap/Vec, not HashMap for ordered fields); Merkle-tree composition from nodes through edges to function root hashes |

</phase_requirements>

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| [rusqlite](https://crates.io/crates/rusqlite) | 0.38.0 | SQLite bindings for Rust | De facto standard (44M+ downloads), ergonomic sync API, bundled SQLite option, WAL mode, transactions, prepared statement caching |
| [rusqlite_migration](https://crates.io/crates/rusqlite_migration) | 2.4.0 | Schema migrations for rusqlite | Lightweight, no macros, uses user_version pragma (faster than migration tables), atomic migration application |
| [blake3](https://crates.io/crates/blake3) | 1.8.3 | Content hashing | Fastest secure hash (SIMD-accelerated), Merkle-tree internal design, 256-bit output, deterministic, public domain |
| [serde_json](https://crates.io/crates/serde_json) | 1.0.x | JSON serialization for complex column types | Already a dependency in lmlang-core, canonical JSON for storing enum variants in TEXT columns |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| [serde](https://crates.io/crates/serde) | 1.0.x | Serialization framework | Already in lmlang-core; used for JSON serialization of complex types to/from SQLite TEXT columns |
| [thiserror](https://crates.io/crates/thiserror) | 2.0.x | Error type definitions | Already in lmlang-core; used for StorageError enum with #[from] for rusqlite::Error wrapping |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| rusqlite | sqlx | sqlx is async-first and heavier; rusqlite is sync and perfect for embedded SQLite with no server |
| rusqlite_migration | refinery | refinery is more powerful (supports Postgres, MySQL) but heavier; rusqlite_migration is purpose-built for rusqlite |
| blake3 | sha2 (SHA-256) | SHA-256 is more widely recognized but 5-10x slower than blake3; blake3 is recommended by RustCrypto for new projects |
| serde_json TEXT columns | borsh binary BLOB columns | borsh is deterministic and faster, but JSON TEXT is human-readable, queryable with SQLite JSON functions, and debuggable; the types already derive Serialize/Deserialize |

**Installation:**
```toml
[dependencies]
rusqlite = { version = "0.38", features = ["bundled"] }
rusqlite_migration = "2.4"
blake3 = "1.8"
# serde, serde_json, thiserror already in workspace
```

## Architecture Patterns

### Recommended Crate Structure

```
crates/
├── lmlang-core/           # Phase 1: types, ops, edges, nodes, graph (unchanged)
└── lmlang-storage/        # Phase 2: NEW crate
    ├── Cargo.toml
    └── src/
        ├── lib.rs          # Public API exports
        ├── error.rs        # StorageError enum
        ├── traits.rs       # GraphStore trait definition
        ├── schema.rs       # SQL schema constants and migration definitions
        ├── sqlite.rs       # SqliteStore implementation
        ├── memory.rs       # InMemoryStore implementation
        ├── hash.rs         # Content hashing (blake3 + canonical serialization)
        └── convert.rs      # ProgramGraph <-> storage format conversion
```

### Pattern 1: Sync GraphStore Trait

**What:** Define `GraphStore` as a synchronous trait (not async). All methods return `Result<T, StorageError>`.

**Why sync over async:** SQLite is inherently synchronous (single-writer, file-based). The `rusqlite` crate is synchronous. Making the trait async would force all callers into an async runtime even when using the in-memory backend, adding complexity with no benefit. If a future backend needs async (e.g., a remote database), a separate `AsyncGraphStore` trait can be introduced then -- the current codebase has zero async dependencies and adding tokio/async-std now would be premature.

**Confidence:** HIGH -- verified that rusqlite is sync-only, project has no async runtime, and Rust async fn in traits (stable since 1.75) still has dyn dispatch limitations.

**Example:**
```rust
pub trait GraphStore {
    // -- Program-level operations --
    fn create_program(&mut self, name: &str) -> Result<ProgramId, StorageError>;
    fn load_program(&self, id: ProgramId) -> Result<ProgramGraph, StorageError>;
    fn delete_program(&mut self, id: ProgramId) -> Result<(), StorageError>;
    fn list_programs(&self) -> Result<Vec<ProgramSummary>, StorageError>;

    // -- Low-level CRUD: Nodes --
    fn insert_node(&mut self, program: ProgramId, node: &ComputeNode, node_id: NodeId) -> Result<(), StorageError>;
    fn get_node(&self, program: ProgramId, id: NodeId) -> Result<ComputeNode, StorageError>;
    fn update_node(&mut self, program: ProgramId, id: NodeId, node: &ComputeNode) -> Result<(), StorageError>;
    fn delete_node(&mut self, program: ProgramId, id: NodeId) -> Result<(), StorageError>;

    // -- Low-level CRUD: Edges --
    fn insert_edge(&mut self, program: ProgramId, edge_id: EdgeId, from: NodeId, to: NodeId, edge: &FlowEdge) -> Result<(), StorageError>;
    fn get_edge(&self, program: ProgramId, id: EdgeId) -> Result<(NodeId, NodeId, FlowEdge), StorageError>;
    fn delete_edge(&mut self, program: ProgramId, id: EdgeId) -> Result<(), StorageError>;

    // -- Low-level CRUD: Types --
    fn insert_type(&mut self, program: ProgramId, type_id: TypeId, ty: &LmType) -> Result<(), StorageError>;
    fn get_type(&self, program: ProgramId, id: TypeId) -> Result<LmType, StorageError>;

    // -- Low-level CRUD: Functions --
    fn insert_function(&mut self, program: ProgramId, func: &FunctionDef) -> Result<(), StorageError>;
    fn get_function(&self, program: ProgramId, id: FunctionId) -> Result<FunctionDef, StorageError>;
    fn update_function(&mut self, program: ProgramId, func: &FunctionDef) -> Result<(), StorageError>;

    // -- Low-level CRUD: Modules --
    fn insert_module(&mut self, program: ProgramId, module: &ModuleDef) -> Result<(), StorageError>;
    fn get_module(&self, program: ProgramId, id: ModuleId) -> Result<ModuleDef, StorageError>;

    // -- Query methods --
    fn find_nodes_by_owner(&self, program: ProgramId, owner: FunctionId) -> Result<Vec<(NodeId, ComputeNode)>, StorageError>;
    fn find_edges_from(&self, program: ProgramId, from: NodeId) -> Result<Vec<(EdgeId, NodeId, FlowEdge)>, StorageError>;
    fn find_edges_to(&self, program: ProgramId, to: NodeId) -> Result<Vec<(EdgeId, NodeId, FlowEdge)>, StorageError>;
    fn find_functions_in_module(&self, program: ProgramId, module: ModuleId) -> Result<Vec<FunctionDef>, StorageError>;
}
```

### Pattern 2: ProgramGraph <-> Storage Conversion (Decompose/Recompose)

**What:** Extract normalized rows from ProgramGraph for storage, and reconstruct ProgramGraph from stored rows on load.

**Why:** ProgramGraph contains petgraph StableGraphs which are complex internal structures. Rather than trying to serialize the StableGraph directly to SQL, decompose it into flat rows (node table, edge table) and reconstruct the StableGraph on load by re-adding nodes and edges in index order.

**Key insight:** petgraph `StableGraph` node indices are stable u32 values. When decomposing, record each node's index. When recomposing, add nodes in index order, which preserves the original `NodeIndex` values -- this is critical because `NodeId`, `EdgeId`, `FunctionDef.entry_node`, and `ComputeOp::Call { target }` all reference these indices.

**Example:**
```rust
/// Decompose a ProgramGraph into flat storage rows.
pub fn decompose(graph: &ProgramGraph) -> StorageRows {
    let mut nodes = Vec::new();
    for idx in graph.compute().node_indices() {
        let node = &graph.compute()[idx];
        nodes.push((NodeId::from(idx), node.clone()));
    }

    let mut edges = Vec::new();
    for edge_ref in graph.compute().edge_references() {
        edges.push((
            EdgeId(edge_ref.id().index() as u32),
            NodeId::from(edge_ref.source()),
            NodeId::from(edge_ref.target()),
            edge_ref.weight().clone(),
        ));
    }

    // Similarly for semantic graph, types, functions, modules...
    StorageRows { nodes, edges, /* ... */ }
}

/// Recompose a ProgramGraph from flat storage rows.
pub fn recompose(rows: StorageRows) -> Result<ProgramGraph, StorageError> {
    // Sort nodes by NodeId to ensure stable index assignment
    // Add nodes to StableGraph in order
    // Add edges referencing the correct NodeIndex values
    // Rebuild TypeRegistry, ModuleTree, function map
    // ...
}
```

### Pattern 3: Transaction Batching for Auto-Persist

**What:** Each mutation method on SqliteStore writes immediately to SQLite, but groups related operations (e.g., insert_node + insert_edges for a new node with connections) within a single transaction.

**Why:** The user decision is auto-persist (every mutation writes through). However, naive per-statement commits are slow. SQLite transactions batch multiple statements into a single fsync. The strategy: each public trait method that performs a single logical operation uses one transaction internally.

**Confidence:** HIGH -- verified that rusqlite transactions with WAL mode achieve 15k+ inserts/sec.

**Example:**
```rust
impl GraphStore for SqliteStore {
    fn insert_node(&mut self, program: ProgramId, node: &ComputeNode, node_id: NodeId) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO compute_nodes (program_id, node_id, owner_function_id, op_json) VALUES (?1, ?2, ?3, ?4)"
            )?;
            let op_json = serde_json::to_string(&node.op)?;
            stmt.execute(params![program.0, node_id.0, node.owner.0, op_json])?;
        }
        tx.commit()?;
        Ok(())
    }
}
```

### Pattern 4: Content Hashing via Canonical Serialization + blake3

**What:** Compute deterministic content hashes for graph nodes by serializing node content to canonical bytes and hashing with blake3. For Merkle-tree composition, a node's hash includes its own content hash plus the sorted hashes of its edge targets.

**Why:** The user decided on Merkle-tree style hashing with eager recomputation and hashes NOT stored in the database. This means hashes are pure derived state computed from the stored content.

**Critical detail -- deterministic serialization:** The existing types all use `Vec`, `IndexMap` (insertion-ordered), and primitive types. There are NO `HashMap` fields in ComputeNode, FlowEdge, or ComputeOp -- so `serde_json::to_string()` produces deterministic output for these types. However, `ProgramGraph` itself contains `HashMap<FunctionId, FunctionDef>` and `HashMap<ModuleId, NodeIndex>` -- these are NOT deterministic. Since hashing is per-node and per-function (not whole-program), this is fine: we hash individual nodes and edges, which are deterministic.

**Confidence:** HIGH -- verified that all hashable types (ComputeNode, FlowEdge, ComputeOp, LmType, ConstValue) use only Vec, IndexMap, and primitives.

**Example:**
```rust
use blake3::Hasher;

/// Hash a single compute node's content (excluding edges).
fn hash_node_content(node: &ComputeNode) -> blake3::Hash {
    let mut hasher = Hasher::new();
    // Serialize op deterministically
    let op_bytes = serde_json::to_vec(&node.op).expect("op serialization cannot fail");
    hasher.update(&op_bytes);
    // Include owner function ID
    hasher.update(&node.owner.0.to_le_bytes());
    hasher.finalize()
}

/// Hash a node including its outgoing edge targets (Merkle composition).
fn hash_node_with_edges(
    node: &ComputeNode,
    outgoing_edges: &[(EdgeId, NodeId, FlowEdge)],
    target_hashes: &HashMap<NodeId, blake3::Hash>,
) -> blake3::Hash {
    let mut hasher = Hasher::new();

    // 1. Hash node content
    hasher.update(hash_node_content(node).as_bytes());

    // 2. Sort edges by (target_port, target_node_id) for deterministic order
    let mut sorted_edges: Vec<_> = outgoing_edges.iter().collect();
    sorted_edges.sort_by_key(|(_, target, edge)| {
        let port = match edge {
            FlowEdge::Data { target_port, .. } => *target_port as u32,
            FlowEdge::Control { branch_index } => branch_index.unwrap_or(0) as u32 + 0x10000,
        };
        (port, target.0)
    });

    // 3. Include edge content and target hashes
    for (_, target_id, edge) in &sorted_edges {
        let edge_bytes = serde_json::to_vec(edge).expect("edge serialization cannot fail");
        hasher.update(&edge_bytes);
        if let Some(target_hash) = target_hashes.get(target_id) {
            hasher.update(target_hash.as_bytes());
        }
    }

    hasher.finalize()
}

/// Compute per-function root hash by hashing all nodes in topological order.
fn hash_function(graph: &ProgramGraph, func_id: FunctionId) -> blake3::Hash {
    // Get nodes owned by this function
    // Topologically sort them (data flow DAG)
    // Hash leaf nodes first, then compose upward
    // Return the hash of the entry node (or combine all node hashes)
    todo!()
}
```

### Pattern 5: StableGraph Index Preservation on Load

**What:** When loading a ProgramGraph from the database, nodes must be added to the StableGraph in a way that preserves their original NodeIndex values, because many structures reference nodes by index.

**Why:** `NodeId(n)` converts to `NodeIndex::new(n)`. If we add nodes out of order or skip indices, the mapping breaks. StableGraph assigns indices sequentially (0, 1, 2, ...) for new nodes. If the stored graph has gaps (from node removals), we must account for that.

**Strategy:** Add placeholder nodes for gaps, then remove them. Or add nodes in order and verify indices match. StableGraph's internal structure is essentially a Vec with Option entries, so adding in index order should produce correct indices.

**Confidence:** MEDIUM -- petgraph's StableGraph does not expose a "force this index" API. Need to verify during implementation that sequential adds produce sequential indices. If not, may need to use serde deserialization of the StableGraph directly for index preservation, or use a thin wrapper.

**Mitigation:** The existing `ProgramGraph` already derives `Serialize`/`Deserialize`. As a fallback, we could serialize the StableGraphs as JSON blobs rather than decomposing into relational rows. But this would lose the ability to query individual nodes/edges in SQL. The recommended approach is to test sequential insertion behavior during implementation and add gap-filling logic if needed.

### Anti-Patterns to Avoid

- **Storing petgraph StableGraph as a single JSON blob:** Loses all query capability, defeats the purpose of normalized relational tables, and makes incremental saves impossible.
- **Using HashMap for any data that will be content-hashed:** Iteration order is randomized. All hashable structures must use Vec, IndexMap, BTreeMap, or sorted iteration.
- **Making the in-memory backend a thin wrapper over SQLite in-memory mode:** The in-memory backend should be a pure Rust HashMap/Vec implementation. SQLite `:memory:` would work but adds unnecessary SQLite overhead for tests and ephemeral sessions.
- **Async trait for embedded SQLite:** Adds runtime dependency (tokio), boxed future allocations, and complexity with zero benefit for a synchronous file-based database.
- **Storing content hashes in the database:** Per user decision, hashes are derived from stored content. Storing them creates a stale cache problem and violates the "DB is source of truth" principle.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SQLite bindings | Raw FFI to sqlite3 | `rusqlite` 0.38.0 | Ergonomic API, type safety, memory safety, prepared statement caching |
| Schema migrations | Custom version tracking | `rusqlite_migration` 2.4 | Atomic migrations, user_version pragma, battle-tested |
| Content hashing | Custom hash function | `blake3` 1.8.3 | SIMD-optimized, Merkle-tree design, cryptographically secure, 6x faster than SHA-256 |
| Enum serialization to SQL | Manual string matching for every op variant | `serde_json` with existing derives | All types already derive Serialize/Deserialize; JSON round-trips are tested in Phase 1 |
| Error type boilerplate | Manual Error/Display impls | `thiserror` 2.0 | Already in workspace, #[from] for wrapping rusqlite::Error, serde_json::Error |

**Key insight:** The Phase 1 data model already has comprehensive serde derives and JSON round-trip tests. Leveraging this for SQL storage (JSON TEXT columns) avoids writing manual ToSql/FromSql implementations for ~30 enum variants across ComputeOp, StructuredOp, FlowEdge, SemanticEdge, LmType, and ConstValue.

## Common Pitfalls

### Pitfall 1: StableGraph Index Gaps on Reconstruction

**What goes wrong:** After removing nodes from a StableGraph, indices have gaps (e.g., nodes 0, 2, 5 exist but 1, 3, 4 are vacant). Loading from the database must reproduce these gaps, or all NodeId/EdgeId references break.

**Why it happens:** StableGraph preserves indices across removals (that's its purpose), but when reconstructing from scratch, `add_node()` assigns sequential indices starting from 0.

**How to avoid:** Either (a) add nodes in index order, inserting dummy nodes for gaps and then removing them, (b) use petgraph's serde deserialization for the StableGraph portion (it handles gaps), or (c) track the maximum node index and pre-allocate. Test with a graph that has had deletions.

**Warning signs:** Tests pass with fresh graphs but fail when loading a graph that previously had node removals.

### Pitfall 2: Non-Deterministic Hashing from HashMap Iteration

**What goes wrong:** Content hashes differ across runs or platforms because a HashMap's iteration order is randomized.

**Why it happens:** Rust's default HashMap uses RandomState (SipHash with random keys). Even `FxHashMap` has non-deterministic iteration order.

**How to avoid:** Never iterate a HashMap when computing hashes. The Phase 1 types are clean -- `ComputeNode`, `FlowEdge`, `LmType`, `ConstValue` all use Vec/IndexMap. But `ProgramGraph.functions` is `HashMap<FunctionId, FunctionDef>`. When computing function-level hashes, iterate functions in sorted FunctionId order, not HashMap iteration order.

**Warning signs:** Hash values change between process restarts with identical data.

### Pitfall 3: SQLite Write-Ahead Log Growing Unbounded

**What goes wrong:** The WAL file grows without bound, consuming disk space and degrading read performance.

**Why it happens:** SQLite checkpoints the WAL every 1000 pages by default, but only if no readers are active. Long-running read connections prevent checkpointing.

**How to avoid:** (a) Close read connections promptly or use short-lived connections. (b) Call `PRAGMA wal_checkpoint(TRUNCATE)` on application startup and periodically. (c) Set `PRAGMA wal_autocheckpoint = 1000` (the default, but verify it's not disabled).

**Warning signs:** WAL file grows to many times the size of the main database file.

### Pitfall 4: Forgetting to Wrap Related Operations in Transactions

**What goes wrong:** A crash between inserting a node and its edges leaves the database in an inconsistent state (orphaned node or dangling edge reference).

**Why it happens:** Without explicit transactions, each SQL statement auto-commits. If the process crashes mid-operation, partial writes are persisted.

**How to avoid:** Every logical operation that touches multiple rows must use `conn.transaction()`. The auto-persist design means every trait method call is a transaction boundary.

**Warning signs:** Database inconsistencies after crashes during testing (use `kill -9` testing).

### Pitfall 5: JSON Serialization Size for Large Programs

**What goes wrong:** Storing every op node as a full JSON string creates significant storage overhead for large programs (thousands of nodes).

**Why it happens:** JSON is verbose -- `{"Core":{"BinaryArith":{"op":"Add"}}}` is 42 bytes for a single Add operation that could be represented in 2-3 bytes.

**How to avoid:** This is acceptable for Phase 2. If storage size becomes a problem, migrate to compact binary serialization (borsh) in a later optimization phase. JSON TEXT provides crucial debuggability during development. Monitor database sizes and set a threshold (e.g., >100MB for a single program) as the trigger for optimization.

**Warning signs:** Database files growing beyond expectations relative to program complexity.

### Pitfall 6: Semantic Graph Reconstruction Ordering

**What goes wrong:** When loading the semantic graph, Contains edges reference parent/child node indices that haven't been added yet.

**Why it happens:** SemanticNode::Module and SemanticNode::Function are added to the semantic StableGraph, and Contains edges connect them. If modules and functions are loaded out of order, edge source/target indices may not exist yet.

**How to avoid:** Load semantic nodes first (all modules, then all functions, then all type defs), then load semantic edges. Within each category, load in index order.

**Warning signs:** Panics or errors when adding edges to non-existent node indices during load.

## Code Examples

### Example 1: SQLite Schema Definition

```sql
-- Migration 1: Initial schema
PRAGMA foreign_keys = ON;

CREATE TABLE programs (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE types (
    program_id  INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    type_id     INTEGER NOT NULL,  -- TypeId.0
    type_json   TEXT NOT NULL,      -- serde_json serialization of LmType
    name        TEXT,               -- for named types (structs, enums); NULL for anonymous
    PRIMARY KEY (program_id, type_id)
);

CREATE TABLE modules (
    program_id  INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    module_id   INTEGER NOT NULL,  -- ModuleId.0
    name        TEXT NOT NULL,
    parent_id   INTEGER,           -- NULL for root module
    visibility  TEXT NOT NULL,      -- "Public" or "Private"
    PRIMARY KEY (program_id, module_id),
    FOREIGN KEY (program_id, parent_id) REFERENCES modules(program_id, module_id)
);

CREATE TABLE functions (
    program_id      INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    function_id     INTEGER NOT NULL,  -- FunctionId.0
    name            TEXT NOT NULL,
    module_id       INTEGER NOT NULL,
    visibility      TEXT NOT NULL,
    params_json     TEXT NOT NULL,      -- JSON array of [name, TypeId] pairs
    return_type_id  INTEGER NOT NULL,   -- TypeId.0
    entry_node_id   INTEGER,            -- NodeId.0 or NULL
    is_closure       INTEGER NOT NULL DEFAULT 0,
    parent_function INTEGER,            -- FunctionId.0 or NULL
    captures_json   TEXT NOT NULL DEFAULT '[]',  -- JSON array of Capture
    PRIMARY KEY (program_id, function_id),
    FOREIGN KEY (program_id, module_id) REFERENCES modules(program_id, module_id)
);

CREATE TABLE compute_nodes (
    program_id   INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    node_id      INTEGER NOT NULL,  -- NodeId.0 (= petgraph NodeIndex)
    owner_fn_id  INTEGER NOT NULL,  -- FunctionId.0
    op_json      TEXT NOT NULL,      -- serde_json serialization of ComputeNodeOp
    PRIMARY KEY (program_id, node_id),
    FOREIGN KEY (program_id, owner_fn_id) REFERENCES functions(program_id, function_id)
);

CREATE TABLE flow_edges (
    program_id   INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    edge_id      INTEGER NOT NULL,  -- EdgeId.0 (= petgraph EdgeIndex)
    source_id    INTEGER NOT NULL,  -- NodeId.0
    target_id    INTEGER NOT NULL,  -- NodeId.0
    edge_json    TEXT NOT NULL,      -- serde_json serialization of FlowEdge
    PRIMARY KEY (program_id, edge_id),
    FOREIGN KEY (program_id, source_id) REFERENCES compute_nodes(program_id, node_id),
    FOREIGN KEY (program_id, target_id) REFERENCES compute_nodes(program_id, node_id)
);

CREATE TABLE semantic_nodes (
    program_id   INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    node_idx     INTEGER NOT NULL,  -- petgraph NodeIndex for semantic graph
    node_json    TEXT NOT NULL,      -- serde_json serialization of SemanticNode
    PRIMARY KEY (program_id, node_idx)
);

CREATE TABLE semantic_edges (
    program_id   INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
    edge_idx     INTEGER NOT NULL,
    source_idx   INTEGER NOT NULL,
    target_idx   INTEGER NOT NULL,
    edge_type    TEXT NOT NULL,      -- "Contains", "Calls", "UsesType"
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
```

### Example 2: StorageError Type

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),

    #[error("program not found: id={0}")]
    ProgramNotFound(i64),

    #[error("node not found: program={program}, node={node}")]
    NodeNotFound { program: i64, node: u32 },

    #[error("edge not found: program={program}, edge={edge}")]
    EdgeNotFound { program: i64, edge: u32 },

    #[error("function not found: program={program}, function={function}")]
    FunctionNotFound { program: i64, function: u32 },

    #[error("module not found: program={program}, module={module}")]
    ModuleNotFound { program: i64, module: u32 },

    #[error("type not found: program={program}, type_id={type_id}")]
    TypeNotFound { program: i64, type_id: u32 },

    #[error("data integrity error: {reason}")]
    IntegrityError { reason: String },

    #[error("graph reconstruction error: {reason}")]
    ReconstructionError { reason: String },
}
```

### Example 3: Rusqlite Migration Setup

```rust
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

const MIGRATIONS: Migrations<'static> = Migrations::from_slice(&[
    M::up(include_str!("migrations/001_initial_schema.sql")),
    // Future migrations added here as new M::up(...) entries
]);

pub fn open_database(path: &str) -> Result<Connection, StorageError> {
    let mut conn = Connection::open(path)?;

    // Enable WAL mode for concurrent reads + single writer performance
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;

    // Apply pending migrations
    MIGRATIONS.to_latest(&mut conn)?;

    Ok(conn)
}
```

### Example 4: InMemoryStore Skeleton

```rust
use std::collections::HashMap;

pub struct InMemoryStore {
    programs: HashMap<ProgramId, ProgramData>,
    next_program_id: i64,
}

struct ProgramData {
    name: String,
    nodes: HashMap<NodeId, ComputeNode>,
    edges: HashMap<EdgeId, (NodeId, NodeId, FlowEdge)>,
    types: Vec<(TypeId, LmType)>,       // ordered by TypeId
    functions: HashMap<FunctionId, FunctionDef>,
    modules: HashMap<ModuleId, ModuleDef>,
    semantic_nodes: HashMap<u32, SemanticNode>,   // indexed by NodeIndex
    semantic_edges: HashMap<u32, (u32, u32, SemanticEdge)>,  // indexed by EdgeIndex
    // Module tree metadata
    module_children: HashMap<ModuleId, Vec<ModuleId>>,
    module_functions: HashMap<ModuleId, Vec<FunctionId>>,
    module_type_defs: HashMap<ModuleId, Vec<TypeId>>,
}

impl GraphStore for InMemoryStore {
    fn create_program(&mut self, name: &str) -> Result<ProgramId, StorageError> {
        let id = ProgramId(self.next_program_id);
        self.next_program_id += 1;
        self.programs.insert(id, ProgramData::new(name));
        Ok(id)
    }

    fn load_program(&self, id: ProgramId) -> Result<ProgramGraph, StorageError> {
        let data = self.programs.get(&id)
            .ok_or(StorageError::ProgramNotFound(id.0))?;
        data.to_program_graph()
    }

    // ... remaining methods follow the same pattern
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `rusqlite` raw version tracking | `rusqlite_migration` with user_version | Available since ~2022, mature by 2025 | No migration table overhead; atomic migrations |
| SHA-256 for content hashing | BLAKE3 (or SHA-256 for compatibility) | BLAKE3 1.0 released 2023 | 5-10x faster, SIMD-accelerated, Merkle-tree internally |
| `async_trait` crate for async traits | Native `async fn` in traits (Rust 1.75+) | Stable since Dec 2023 | No more Box allocations; but dyn dispatch still needs async_trait |
| `rusqlite` 0.30-0.35 | `rusqlite` 0.38.0 (Dec 2025) | Breaking: u64/usize ToSql disabled by default, min SQLite 3.34.1 | Better safety defaults; bundled SQLite 3.51.1 |
| `borsh` with indexmap feature on indexmap crate | `borsh` with indexmap feature on borsh crate | borsh 1.5.6 (2025) | Fixes cyclic dependency issue |

**Deprecated/outdated:**
- `barrel` for programmatic schema generation: unmaintained, last release 2021. Use raw SQL migration strings instead.
- `diesel` for SQLite ORM: overkill for this use case; we need fine-grained control over schema and queries.
- `async_trait` crate: still useful for dyn dispatch, but native async fn in traits preferred where dyn dispatch is not needed.

## Open Questions

1. **StableGraph index gap handling**
   - What we know: StableGraph preserves gaps from node removals. Sequential `add_node()` assigns indices 0, 1, 2, ...
   - What's unclear: Whether adding then removing placeholder nodes reliably creates gaps at the right indices. Whether serde roundtrip of just the StableGraph is a viable shortcut.
   - Recommendation: Start with the decompose/recompose approach. If index gaps cause issues, fall back to serializing the StableGraph sub-structures via serde JSON, storing them as a blob alongside the relational rows (hybrid approach). Test with graphs that have had deletions.

2. **ProgramId type design**
   - What we know: Multi-program database needs a program identifier. SQLite uses INTEGER PRIMARY KEY with auto-increment.
   - What's unclear: Whether ProgramId should be defined in lmlang-core (making core aware of storage) or in lmlang-storage (cleaner separation but requires conversion).
   - Recommendation: Define ProgramId in lmlang-storage. It's a storage concern. The GraphStore trait owns ProgramId; lmlang-core remains storage-agnostic.

3. **Dirty tracking for incremental saves**
   - What we know: User decided on incremental saves (track dirty nodes/edges, write only changes). With auto-persist, every mutation writes through immediately.
   - What's unclear: If every mutation writes through immediately, what does "incremental save" mean? It may be that auto-persist IS the incremental save -- each mutation is a small write.
   - Recommendation: Treat auto-persist as the incremental save mechanism. Each `insert_node()`, `update_node()`, etc. writes one row. The "save_program" high-level method is for initial bulk save of a newly created program. Dirty tracking becomes relevant only if we later add a "deferred write" mode.

4. **Semantic graph storage granularity**
   - What we know: The semantic graph is a StableGraph with SemanticNode (Module/Function/TypeDef) and SemanticEdge (Contains/Calls/UsesType). It mirrors some data in the modules/functions tables.
   - What's unclear: Whether to store the semantic graph in its own tables (semantic_nodes, semantic_edges) or derive it from the modules/functions/types tables on load.
   - Recommendation: Store in dedicated semantic tables. The semantic graph's NodeIndex values must be preserved, and the graph structure (especially edge connectivity) would be expensive to derive. The redundancy with modules/functions tables is acceptable since the semantic graph adds connectivity information that modules/functions tables don't capture (e.g., Calls edges, UsesType edges).

## Sources

### Primary (HIGH confidence)
- [rusqlite crates.io](https://crates.io/crates/rusqlite) - Version 0.38.0, features, bundled SQLite 3.51.1
- [rusqlite docs.rs](https://docs.rs/rusqlite/latest/rusqlite/) - Connection, Transaction, Statement, ToSql/FromSql APIs
- [rusqlite_migration docs.rs](https://docs.rs/rusqlite_migration/latest/rusqlite_migration/) - Migrations struct, M, user_version approach, from-directory feature
- [blake3 crates.io](https://crates.io/crates/blake3) - Version 1.8.3, SIMD acceleration, Merkle-tree internal design
- [borsh docs.rs](https://docs.rs/borsh/latest/borsh/) - Version 1.6.0, deterministic serialization, indexmap feature
- [petgraph docs.rs](https://docs.rs/petgraph/latest/petgraph/stable_graph/struct.StableGraph.html) - StableGraph API, node_indices, edge_references, serde-1 feature

### Secondary (MEDIUM confidence)
- [15k inserts/s with Rust and SQLite](https://kerkour.com/high-performance-rust-with-sqlite) - WAL mode, transaction batching, prepared statements benchmarks
- [Rust Async Project Goals 2025 H1](https://rust-lang.github.io/rust-project-goals/2025h1/async.html) - State of async fn in traits, dyn dispatch limitations
- [The stable HashMap trap](https://morestina.net/1843/the-stable-hashmap-trap) - Why HashMap iteration order is non-deterministic, how to handle for hashing
- [rusqlite_migration GitHub](https://github.com/cljoly/rusqlite_migration) - user_version approach rationale, performance characteristics

### Tertiary (LOW confidence)
- Training data knowledge on petgraph StableGraph index behavior during reconstruction (needs validation during implementation)
- Training data knowledge on rusqlite Connection thread safety model (verified against docs but not tested)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all libraries verified on crates.io/docs.rs with current versions and active maintenance
- Architecture: HIGH - patterns derived from existing codebase analysis (Phase 1 types, serde derives, petgraph usage) and verified library APIs
- Pitfalls: HIGH/MEDIUM - HashMap non-determinism and WAL growth are well-documented; StableGraph index reconstruction is MEDIUM confidence (needs implementation validation)
- Content hashing: HIGH - blake3 is well-established, canonical serialization approach verified against actual type definitions in codebase

**Research date:** 2026-02-18
**Valid until:** 2026-04-18 (stable domain, mature libraries)

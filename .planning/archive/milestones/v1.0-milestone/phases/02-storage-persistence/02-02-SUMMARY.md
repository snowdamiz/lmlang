---
phase: 02-storage-persistence
plan: 02
subsystem: database
tags: [storage, sqlite, rusqlite, migrations, wal, transactions, graphstore]

# Dependency graph
requires:
  - phase: 02-storage-persistence
    plan: 01
    provides: GraphStore trait, decompose/recompose, InMemoryStore, ProgramId, StorageError
provides:
  - SqliteStore implementing full GraphStore trait with 26 methods
  - Normalized relational schema with 8 tables and 5 indices
  - Automatic schema migrations via rusqlite_migration
  - WAL mode + foreign keys + transaction-wrapped writes
  - Save/load roundtrip fidelity for multi-function closure programs
affects: [02-03-content-hashing, 04-agent-tool-api]

# Tech tracking
tech-stack:
  added: [rusqlite 0.32, rusqlite_migration 1.3]
  patterns: [JSON TEXT columns for complex enums, transaction-per-method auto-persist, include_str! embedded migrations]

key-files:
  created:
    - crates/lmlang-storage/src/sqlite.rs
    - crates/lmlang-storage/src/schema.rs
    - crates/lmlang-storage/src/migrations/001_initial_schema.sql
  modified:
    - crates/lmlang-storage/Cargo.toml
    - crates/lmlang-storage/src/lib.rs
    - crates/lmlang-storage/src/error.rs

key-decisions:
  - "rusqlite 0.32 (not 0.38) to match rusqlite_migration 1.3 compatibility"
  - "Explicit child-table DELETE ordering before program deletion (not relying on CASCADE alone)"
  - "ModuleTree rebuilt from stored modules + functions during load (not serialized as blob)"
  - "Semantic index maps (module_semantic_indices, function_semantic_indices) derived from semantic node content on load"

patterns-established:
  - "JSON TEXT columns for all complex Rust enums (ComputeNodeOp, FlowEdge, FunctionDef params/captures, SemanticNode)"
  - "Transaction-per-method: every GraphStore write method wraps its SQL in conn.transaction()"
  - "Schema migrations via include_str! embedded SQL files with rusqlite_migration"
  - "save_decomposed: DELETE-all-then-INSERT pattern for full program overwrites"

requirements-completed: [STORE-01]

# Metrics
duration: 6min
completed: 2026-02-18
---

# Phase 02 Plan 02: SQLite Persistence Backend Summary

**SqliteStore with WAL mode, normalized relational schema, atomic transactions, and 8 roundtrip tests proving multi-function closure programs survive save/load to SQLite**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-18T07:11:13Z
- **Completed:** 2026-02-18T07:17:30Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Implemented SqliteStore with all 26 GraphStore trait methods: program CRUD, node/edge/type/function/module CRUD, semantic CRUD, and 6 query methods
- Created normalized SQL schema with 8 tables (programs, types, modules, functions, compute_nodes, flow_edges, semantic_nodes, semantic_edges) and 5 indices
- Full program roundtrip test: 3-function closure program with 11 nodes, 8 edges, 4 semantic nodes, 3 semantic edges survives SQLite save/load with all data intact
- 8 new integration tests covering program lifecycle, individual CRUD operations, owner-based queries, and overwrite behavior

## Task Commits

Each task was committed atomically:

1. **Task 1: SQL schema, migration setup, and SqliteStore implementation** - `490ae8a` (feat)
2. **Task 2: SQLite save/load roundtrip integration tests** - `6e37f0a` (test)

## Files Created/Modified
- `crates/lmlang-storage/src/migrations/001_initial_schema.sql` - 8-table normalized schema with FK constraints and indices
- `crates/lmlang-storage/src/schema.rs` - Database open with WAL mode, foreign keys, and migration application
- `crates/lmlang-storage/src/sqlite.rs` - SqliteStore implementing full GraphStore + 8 integration tests
- `crates/lmlang-storage/Cargo.toml` - Added rusqlite 0.32 (bundled) and rusqlite_migration 1.3
- `crates/lmlang-storage/src/lib.rs` - Added schema and sqlite module declarations, SqliteStore re-export
- `crates/lmlang-storage/src/error.rs` - Added Sqlite and Migration error variants

## Decisions Made
- Used rusqlite 0.32 instead of 0.38 (from research) because rusqlite_migration 1.3 requires rusqlite 0.32 -- version compatibility constraint
- Explicit child-table DELETE ordering in delete_program rather than relying solely on CASCADE -- clearer and works even if foreign_keys pragma is somehow off
- ModuleTree reconstructed from loaded modules and functions data during load_decomposed, not stored as a separate blob -- maintains normalized schema design
- Semantic index maps (module_semantic_indices, function_semantic_indices) derived from semantic node content by scanning SemanticNode::Module and SemanticNode::Function variants during load -- no extra table needed

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] rusqlite version downgraded from 0.38 to 0.32**
- **Found during:** Task 1 (Cargo.toml dependency setup)
- **Issue:** Research recommended rusqlite 0.38 but rusqlite_migration 1.3 depends on rusqlite 0.32.x; libsqlite3-sys linking conflict prevented build
- **Fix:** Used rusqlite 0.32 with bundled feature to match rusqlite_migration compatibility
- **Files modified:** crates/lmlang-storage/Cargo.toml
- **Verification:** cargo check passes, all APIs used are available in 0.32
- **Committed in:** 490ae8a (Task 1 commit)

**2. [Rule 3 - Blocking] Migrations API adjusted for rusqlite_migration 1.3**
- **Found during:** Task 1 (schema.rs implementation)
- **Issue:** Research showed `Migrations::from_slice` (const fn) but rusqlite_migration 1.3 uses `Migrations::new(Vec<M>)` instead; `to_latest` takes `&mut Connection` not `&mut Connection` with different error type
- **Fix:** Used `Migrations::new(vec![M::up(...)])` and wrapped migration error as `StorageError::Migration(String)` since the error type doesn't implement std::error::Error cleanly for #[from]
- **Files modified:** crates/lmlang-storage/src/schema.rs, crates/lmlang-storage/src/error.rs
- **Verification:** Migrations apply correctly in all 8 SQLite tests
- **Committed in:** 490ae8a (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking -- version compatibility)
**Impact on plan:** Both fixes were necessary due to version differences between research and actual crate compatibility. No functional compromise; all planned features implemented.

## Issues Encountered
None beyond the version compatibility issues documented above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- SqliteStore ready for production use: WAL mode, atomic transactions, auto-migration
- Content hashing (Plan 03) can use SqliteStore as persistence backend
- Both InMemoryStore and SqliteStore pass identical roundtrip tests, confirming GraphStore contract works across backends

## Self-Check: PASSED

All 7 claimed files verified present. Both commit hashes (490ae8a, 6e37f0a) confirmed in git log. 18 total tests passing (10 existing + 8 new SQLite tests).

---
*Phase: 02-storage-persistence*
*Completed: 2026-02-18*

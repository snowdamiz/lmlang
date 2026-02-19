---
phase: 04-ai-agent-tool-api
plan: 02
subsystem: api
tags: [graphql, undo, checkpoint, rusqlite, petgraph, interpreter, typechecker]

# Dependency graph
requires:
  - phase: 04-01
    provides: "API schema types (mutations, queries, verify, simulate, history, diagnostics)"
provides:
  - "ProgramService with mutation/query/verify/simulate/undo methods"
  - "AppState wrapping ProgramService in Arc<Mutex<>>"
  - "EditCommand enum with inverse() for reversible mutations"
  - "EditLog for SQLite-backed persistent edit history"
  - "CheckpointManager for named graph snapshots"
  - "modify_compute_node_op on ProgramGraph"
  - "edit_log and checkpoints SQLite tables"
affects: [04-03, 04-04]

# Tech tracking
tech-stack:
  added: [petgraph (server-side edge traversal)]
  patterns: [clone-and-swap batch atomicity, command-pattern undo, edit log with redo stack]

key-files:
  created:
    - crates/lmlang-server/src/service.rs
    - crates/lmlang-server/src/state.rs
    - crates/lmlang-server/src/undo.rs
    - crates/lmlang-storage/src/migrations/002_edit_history.sql
  modified:
    - crates/lmlang-server/src/lib.rs
    - crates/lmlang-server/Cargo.toml
    - crates/lmlang-storage/src/schema.rs
    - crates/lmlang-core/src/graph.rs

key-decisions:
  - "ProgramService owns graph, store, connection, and program_id as a single coordinator"
  - "Batch mutations use clone-and-swap: clone graph, apply all, validate, swap on success"
  - "Single mutations apply to real graph with inverse-revert on validation failure"
  - "EditCommand inverse() computes the inverse command for each variant (LIFO for batches)"
  - "Checkpoint stores serialized ProgramGraph JSON with edit_log position reference"
  - "New mutations clear the redo stack (invalidate undone entries)"
  - "render_edge_ref uses petgraph::stable_graph::EdgeReference for StableGraph compatibility"

patterns-established:
  - "Service pattern: thin handler -> ProgramService method -> graph/storage/checker crates"
  - "Command pattern: EditCommand wraps mutations for recording and replay"
  - "Clone-and-swap: batch atomicity without manual rollback"
  - "Detail levels: Summary/Standard/Full control response verbosity"

requirements-completed: [STORE-03, TOOL-01, TOOL-02, TOOL-03, TOOL-04]

# Metrics
duration: 15min
completed: 2026-02-18
---

# Phase 04 Plan 02: ProgramService and Undo System Summary

**ProgramService coordinator with mutation (single/batch/dry_run), query, verify, simulate, and SQLite-backed undo/redo/checkpoint operations**

## Performance

- **Duration:** ~15 min
- **Started:** 2026-02-18T18:11:00Z
- **Completed:** 2026-02-18T18:26:00Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Built ProgramService as single coordinator between HTTP handlers and graph/storage/checker/interpreter crates
- Implemented full undo/redo system with SQLite-backed edit log, command inversion, and redo stack management
- Created named checkpoint system with graph JSON serialization and restore
- All five agent tool capabilities operational: mutate, query, verify, simulate, undo

## Task Commits

Each task was committed atomically:

1. **Task 1: SQLite migration for edit history and undo system** - `e1b3b1f` (feat)
2. **Task 2: ProgramService implementation with AppState** - `5b9cfbd` (feat)

## Files Created/Modified
- `crates/lmlang-storage/src/migrations/002_edit_history.sql` - edit_log and checkpoints tables
- `crates/lmlang-storage/src/schema.rs` - registered new migration
- `crates/lmlang-server/src/undo.rs` - EditCommand, EditLog, CheckpointManager
- `crates/lmlang-server/src/service.rs` - ProgramService with all 5 capabilities
- `crates/lmlang-server/src/state.rs` - AppState with Arc<Mutex<ProgramService>>
- `crates/lmlang-server/src/lib.rs` - added service, state, undo modules
- `crates/lmlang-server/Cargo.toml` - added petgraph dependency
- `crates/lmlang-core/src/graph.rs` - added modify_compute_node_op method

## Decisions Made
- Used `petgraph::stable_graph::EdgeReference` (not `petgraph::graph::EdgeReference`) since ProgramGraph uses StableGraph
- ProgramService takes `db_path: &str` and opens both Connection and SqliteStore internally
- Checkpoint stores full ProgramGraph as JSON (rather than incremental delta) for simplicity and correctness
- json_to_value helper uses function signature type hints to disambiguate numeric JSON values

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added petgraph dependency to lmlang-server**
- **Found during:** Task 2
- **Issue:** service.rs uses petgraph types (NodeIndex, EdgeIndex, Direction, EdgeRef) for graph traversal but lmlang-server Cargo.toml did not include petgraph
- **Fix:** Added `petgraph = "0.8"` to lmlang-server/Cargo.toml
- **Files modified:** crates/lmlang-server/Cargo.toml
- **Verification:** cargo check -p lmlang-server succeeds
- **Committed in:** 5b9cfbd (Task 2 commit)

**2. [Rule 1 - Bug] Used stable_graph::EdgeReference instead of graph::EdgeReference**
- **Found during:** Task 2
- **Issue:** render_edge_ref initially used petgraph::graph::EdgeReference but ProgramGraph.compute() returns StableGraph, whose edge iterators yield petgraph::stable_graph::EdgeReference
- **Fix:** Changed parameter type to petgraph::stable_graph::EdgeReference
- **Files modified:** crates/lmlang-server/src/service.rs
- **Verification:** cargo check -p lmlang-server succeeds
- **Committed in:** 5b9cfbd (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Both auto-fixes necessary for compilation. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- ProgramService ready for HTTP handler wiring (Plan 03)
- All five tool capabilities (TOOL-01 through TOOL-04 + STORE-03) have service methods
- AppState pattern ready for axum integration

## Self-Check: PASSED

All 5 expected files verified present. Both task commits (e1b3b1f, 5b9cfbd) confirmed in git log.

---
*Phase: 04-ai-agent-tool-api*
*Completed: 2026-02-18*

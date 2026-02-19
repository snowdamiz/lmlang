---
phase: 02-storage-persistence
plan: 01
subsystem: database
tags: [storage, graphstore, in-memory, decompose, recompose, trait]

# Dependency graph
requires:
  - phase: 01-core-graph-data-model
    provides: ProgramGraph, ComputeNode, FlowEdge, SemanticNode, SemanticEdge, TypeRegistry, ModuleTree, FunctionDef
provides:
  - GraphStore trait defining full CRUD + query storage contract
  - InMemoryStore as first-class backend for tests and ephemeral sessions
  - ProgramGraph decompose/recompose for storage layer conversion
  - ProgramId newtype for storage-layer program identity
  - StorageError enum with all storage failure modes
  - ProgramGraph::from_parts constructor enabling storage reconstruction
affects: [02-02-sqlite, 02-03-content-hashing, 04-agent-tool-api]

# Tech tracking
tech-stack:
  added: [petgraph (in lmlang-storage)]
  patterns: [two-layer trait API (CRUD + convenience), decompose/recompose pattern for graph serialization, from_parts constructors for reconstruction]

key-files:
  created:
    - crates/lmlang-storage/Cargo.toml
    - crates/lmlang-storage/src/lib.rs
    - crates/lmlang-storage/src/error.rs
    - crates/lmlang-storage/src/types.rs
    - crates/lmlang-storage/src/traits.rs
    - crates/lmlang-storage/src/convert.rs
    - crates/lmlang-storage/src/memory.rs
  modified:
    - crates/lmlang-core/src/graph.rs
    - crates/lmlang-core/src/type_id.rs
    - crates/lmlang-core/src/module.rs

key-decisions:
  - "Sync GraphStore trait (not async) matching current single-threaded design"
  - "TypeRegistry::from_parts and ModuleTree::from_parts constructors for storage-layer reconstruction"
  - "StableGraph gap-filling with dummy nodes for index preservation during recompose"
  - "InMemoryStore stores DecomposedProgram fields in HashMaps, converts via decompose/recompose for load/save"
  - "find_nodes_by_type implemented via edge value_type filtering rather than node-level type annotation"

patterns-established:
  - "Two-layer trait API: low-level CRUD methods as trait foundation, high-level convenience methods built on top"
  - "Decompose/recompose pattern: ProgramGraph -> DecomposedProgram flat vectors -> ProgramGraph"
  - "from_parts constructors on core types (ProgramGraph, TypeRegistry, ModuleTree) for bypassing builder validation"
  - "StorageError variants per entity type with program ID context"

requirements-completed: [STORE-02]

# Metrics
duration: 8min
completed: 2026-02-18
---

# Phase 02 Plan 01: GraphStore Trait + InMemoryStore Summary

**GraphStore trait with two-layer CRUD+query API, decompose/recompose conversion, and InMemoryStore passing full program roundtrip with closures**

## Performance

- **Duration:** 8 min
- **Started:** 2026-02-18T06:59:15Z
- **Completed:** 2026-02-18T07:07:21Z
- **Tasks:** 2
- **Files modified:** 11

## Accomplishments
- Created lmlang-storage crate with GraphStore trait defining 26 methods across program-level, CRUD, and query operations
- Implemented decompose/recompose that correctly handles StableGraph index gaps via dummy node insertion/removal
- InMemoryStore implements full GraphStore contract: create/save/load/delete programs, node/edge/type/function/module CRUD, semantic CRUD, and all query methods
- Multi-function program with closures survives InMemoryStore save/load roundtrip with all 11 nodes, 8 edges, 3 functions, 4 semantic nodes, and 3 semantic edges intact

## Task Commits

Each task was committed atomically:

1. **Task 1: Create lmlang-storage crate with trait, error types, ProgramId, and ProgramGraph accessors** - `3496f01` (feat)
2. **Task 2: Implement decompose/recompose and InMemoryStore with roundtrip tests** - `dbde86b` (feat)

## Files Created/Modified
- `crates/lmlang-storage/Cargo.toml` - New crate with lmlang-core, petgraph, serde, thiserror dependencies
- `crates/lmlang-storage/src/lib.rs` - Module declarations and re-exports
- `crates/lmlang-storage/src/error.rs` - StorageError enum with 8 variants covering all storage failure modes
- `crates/lmlang-storage/src/types.rs` - ProgramId(i64) newtype and ProgramSummary struct
- `crates/lmlang-storage/src/traits.rs` - GraphStore trait with 26 methods across CRUD, convenience, and query layers
- `crates/lmlang-storage/src/convert.rs` - decompose/recompose with DecomposedProgram intermediate type (3 roundtrip tests)
- `crates/lmlang-storage/src/memory.rs` - InMemoryStore with full GraphStore impl (7 tests including full program roundtrip)
- `crates/lmlang-core/src/graph.rs` - Added from_parts constructor and 4 public accessors
- `crates/lmlang-core/src/type_id.rs` - Added from_parts, iter(), names(), next_id(), len()
- `crates/lmlang-core/src/module.rs` - Added from_parts, all_modules(), children_map(), functions_map(), type_defs_map(), next_id()

## Decisions Made
- Sync GraphStore trait (not async) -- matches current single-threaded design, can be made async later if needed
- TypeRegistry and ModuleTree get `from_parts` constructors -- allows storage layer to reconstruct without going through builder methods that enforce invariants already satisfied by stored data
- StableGraph index gap handling via dummy node insertion then removal -- preserves NodeId/EdgeId mapping through roundtrip
- `find_nodes_by_type` uses edge value_type filtering since types are inferred from edges (not stored on nodes per Phase 1 decision)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added petgraph dependency to lmlang-storage**
- **Found during:** Task 2 (convert.rs implementation)
- **Issue:** convert.rs needs petgraph types (NodeIndex, StableGraph, EdgeRef, IntoEdgeReferences) that aren't re-exported from lmlang-core
- **Fix:** Added `petgraph = { version = "0.8", features = ["serde-1"] }` to lmlang-storage Cargo.toml
- **Files modified:** crates/lmlang-storage/Cargo.toml
- **Verification:** cargo check passes
- **Committed in:** dbde86b (Task 2 commit)

**2. [Rule 2 - Missing Critical] Added TypeRegistry::from_parts and ModuleTree::from_parts constructors**
- **Found during:** Task 2 (recompose implementation)
- **Issue:** Recompose needs to rebuild TypeRegistry and ModuleTree from stored parts, but no constructors exist to bypass the builder pattern
- **Fix:** Added `from_parts` constructors and accessor methods on both types
- **Files modified:** crates/lmlang-core/src/type_id.rs, crates/lmlang-core/src/module.rs
- **Verification:** All 89 lmlang-core tests still pass, recompose roundtrip works
- **Committed in:** dbde86b (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 missing critical)
**Impact on plan:** Both auto-fixes necessary for correct decompose/recompose implementation. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- GraphStore trait ready for SQLite implementation (Plan 02)
- InMemoryStore available as reference implementation and test backend
- Content hashing (Plan 03) can use decompose to access graph components

## Self-Check: PASSED

All 10 claimed files verified present. Both commit hashes (3496f01, dbde86b) confirmed in git log. 89 lmlang-core tests + 10 lmlang-storage tests = 99 tests passing.

---
*Phase: 02-storage-persistence*
*Completed: 2026-02-18*

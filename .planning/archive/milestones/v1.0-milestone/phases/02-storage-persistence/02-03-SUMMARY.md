---
phase: 02-storage-persistence
plan: 03
subsystem: hashing
tags: [blake3, merkle-tree, content-hashing, deterministic, change-detection]

# Dependency graph
requires:
  - phase: 02-storage-persistence
    plan: 01
    provides: ProgramGraph accessors, ComputeNode, FlowEdge, NodeId, FunctionId
provides:
  - blake3 deterministic content hashing for compute nodes (Level 1)
  - Merkle-tree composition with sorted edge keys (Level 2)
  - Per-function root hash computation (Level 3)
  - Bulk hash_all_functions utility
affects: [04-agent-tool-api, 06-incremental-compilation]

# Tech tracking
tech-stack:
  added: [blake3 1.8]
  patterns: [serde_json canonical serialization for hashing, two-pass hash computation (content then composite), deterministic edge sorting by (discriminant, port/index, target_id)]

key-files:
  created:
    - crates/lmlang-storage/src/hash.rs
  modified:
    - crates/lmlang-storage/Cargo.toml
    - crates/lmlang-storage/src/lib.rs

key-decisions:
  - "serde_json::to_vec for canonical op serialization (safe because ComputeNodeOp uses no HashMap)"
  - "Two-pass hash_function: content hashes first, then composite hashes with edges (avoids topological ordering complexity)"
  - "Cross-function edge targets use content-only hash (not composite) since we only hash within function boundary"

patterns-established:
  - "Edge sort key: Data edges by (0, target_port, target_id), Control edges by (1, branch_index, target_id)"
  - "Function root hash: sorted NodeId iteration with (node_id_bytes + composite_hash_bytes) feeding final hasher"

requirements-completed: [STORE-04]

# Metrics
duration: 4min
completed: 2026-02-18
---

# Phase 02 Plan 03: Deterministic Content Hashing Summary

**blake3 content hashing with Merkle-tree composition: node content hashes, edge-aware composite hashes, and per-function root hashes for O(1) cross-function change detection**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-18T07:21:34Z
- **Completed:** 2026-02-18T07:25:40Z
- **Tasks:** 3 (TDD: RED, GREEN, REFACTOR)
- **Files modified:** 3

## Accomplishments
- Implemented 4 public hash functions: hash_node_content, hash_node_with_edges, hash_function, hash_all_functions
- 9 tests covering all 3 levels: node content determinism/sensitivity, Merkle edge composition, function root hash isolation
- Deterministic ordering throughout: edges sorted by (discriminant, port, target_id), nodes sorted by NodeId
- Two-pass approach in hash_function avoids topological sort complexity while still achieving Merkle-style propagation

## Task Commits

Each task was committed atomically:

1. **Task 1 (RED): Failing hash tests** - `9f9b889` (test)
2. **Task 2 (GREEN): Implement hash functions** - `fb1fb3b` (feat)
3. **Task 3 (REFACTOR): Clean up unused import** - `f1686cf` (refactor)

## Files Created/Modified
- `crates/lmlang-storage/src/hash.rs` - 4 public hash functions + edge_sort_key helper + 9 tests (416 lines)
- `crates/lmlang-storage/Cargo.toml` - Added blake3 1.8 dependency
- `crates/lmlang-storage/src/lib.rs` - Added hash module declaration and re-exports

## Decisions Made
- Used serde_json::to_vec for canonical serialization of ComputeNodeOp -- safe because ComputeNodeOp and all its nested types use Vec and enum variants, never HashMap, guaranteeing deterministic JSON output
- Two-pass approach in hash_function (content hashes first, then composite hashes) avoids needing topological sort while still propagating target content changes through edges
- Cross-function edge targets use content-only hash rather than composite hash, since we only compute hashes within function boundaries

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added petgraph::visit::EdgeRef import**
- **Found during:** Task 2 (GREEN implementation)
- **Issue:** petgraph 0.8 requires explicit EdgeRef trait import for .target() and .weight() on edge references from edges_directed()
- **Fix:** Added `use petgraph::visit::EdgeRef;` import
- **Files modified:** crates/lmlang-storage/src/hash.rs
- **Verification:** Compilation succeeds, all 9 tests pass
- **Committed in:** fb1fb3b (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking -- missing trait import)
**Impact on plan:** Trivial import fix. No functional compromise.

## Issues Encountered
None beyond the missing trait import documented above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Content hashing complete: all 4 public functions ready for use
- Phase 02 (Storage & Persistence) is now fully complete (3/3 plans done)
- Agent tool API (Phase 04) can use hash_all_functions for change detection
- 27 total tests passing in lmlang-storage (18 existing + 9 new hash tests)
- Full workspace clean: 0 warnings, 0 errors

## Self-Check: PASSED

All 3 claimed files verified present. All 3 commit hashes (9f9b889, fb1fb3b, f1686cf) confirmed in git log. 27 total tests passing (18 existing + 9 new hash tests).

---
*Phase: 02-storage-persistence*
*Completed: 2026-02-18*

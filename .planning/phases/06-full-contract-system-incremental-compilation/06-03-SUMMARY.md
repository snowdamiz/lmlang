---
phase: 06-full-contract-system-incremental-compilation
plan: 03
subsystem: codegen
tags: [incremental-compilation, dirty-tracking, call-graph, per-function-object-files, blake3]

# Dependency graph
requires:
  - phase: 06-full-contract-system-incremental-compilation (plan 01)
    provides: contract-aware compilation hashing (hash_all_functions_for_compilation)
provides:
  - IncrementalState with per-function hash tracking and persistence
  - RecompilationPlan showing dirty/dependent/cached functions
  - compile_incremental function for incremental builds
  - GET /programs/{id}/dirty API endpoint
  - Per-function .o file compilation and multi-object linking
affects: [06-full-contract-system-incremental-compilation]

# Tech tracking
tech-stack:
  added: [blake3 (settings hash), lmlang-storage (codegen dependency)]
  patterns: [per-function-object-files, extern-runtime-declarations, call-graph-bfs-propagation]

key-files:
  created:
    - crates/lmlang-codegen/src/incremental.rs
  modified:
    - crates/lmlang-codegen/src/compiler.rs
    - crates/lmlang-codegen/src/lib.rs
    - crates/lmlang-codegen/src/linker.rs
    - crates/lmlang-codegen/src/runtime.rs
    - crates/lmlang-codegen/Cargo.toml
    - crates/lmlang-codegen/tests/integration_tests.rs
    - crates/lmlang-server/src/schema/compile.rs
    - crates/lmlang-server/src/handlers/compile.rs
    - crates/lmlang-server/src/service.rs
    - crates/lmlang-server/src/router.rs
    - crates/lmlang-server/tests/integration_test.rs

key-decisions:
  - "Per-function .o files with separate runtime .o to avoid duplicate symbol conflicts"
  - "Extern-only runtime declarations for per-function modules; full body in dedicated runtime.o"
  - "BFS propagation through reverse call graph for transitive dirty dependents"
  - "Lazy IncrementalState initialization in ProgramService (first compile creates it)"

patterns-established:
  - "Per-function compilation: each function compiles to its own .o file with forward-declared cross-module references"
  - "Runtime isolation: declare_runtime_functions_extern for multi-module builds, full body in runtime.o"
  - "Settings hash invalidation: changing opt_level/target/debug clears entire object cache"

requirements-completed: [STORE-05]

# Metrics
duration: 12min
completed: 2026-02-18
---

# Phase 6 Plan 03: Incremental Compilation Engine Summary

**Function-level incremental compilation with BFS dirty propagation, per-function object files, and dirty status API endpoint**

## Performance

- **Duration:** ~12 min
- **Started:** 2026-02-18T16:00:00Z
- **Completed:** 2026-02-18T16:12:00Z
- **Tasks:** 2/2
- **Files modified:** 13

## Accomplishments

- Incremental compilation engine that compiles only dirty functions and their transitive callers
- Contract-only changes are excluded from compilation hashes, preventing unnecessary recompilation
- GET /programs/{id}/dirty API endpoint shows dirty/dependent/cached function status
- Per-function .o file architecture enabling future parallel compilation
- 12 new tests (8 unit + 4 integration) verify correctness, dirty detection, and API behavior

## Task Commits

Each task was committed atomically:

1. **Task 1: Incremental compilation engine** - `f703126` (feat)
2. **Task 2: Dirty query API endpoint** - `4743436` (feat)

**Plan metadata:** (pending)

## Files Created/Modified

- `crates/lmlang-codegen/src/incremental.rs` - IncrementalState, RecompilationPlan, build_call_graph, compute_dirty, settings hash
- `crates/lmlang-codegen/src/compiler.rs` - compile_incremental function with per-function .o emission
- `crates/lmlang-codegen/src/linker.rs` - link_objects for multi-object file linking
- `crates/lmlang-codegen/src/runtime.rs` - declare_runtime_functions_extern for extern-only declarations
- `crates/lmlang-codegen/src/lib.rs` - Module export for incremental, compile_incremental re-export
- `crates/lmlang-codegen/Cargo.toml` - Added lmlang-storage and blake3 dependencies
- `crates/lmlang-codegen/tests/integration_tests.rs` - 4 integration tests for incremental compilation
- `crates/lmlang-server/src/schema/compile.rs` - DirtyStatusResponse, DirtyFunctionView, CachedFunctionView
- `crates/lmlang-server/src/handlers/compile.rs` - dirty_status handler
- `crates/lmlang-server/src/service.rs` - ProgramService.dirty_status(), compile_incremental(), IncrementalState field
- `crates/lmlang-server/src/router.rs` - GET /programs/{id}/dirty route
- `crates/lmlang-server/tests/integration_test.rs` - Dirty status integration test

## Decisions Made

- **Per-function .o files with separate runtime .o:** Each function compiles to its own object file with forward-declared cross-module references. The runtime error handler body lives in a dedicated `runtime.o` to avoid duplicate symbol conflicts at link time.
- **Extern-only runtime declarations:** Created `declare_runtime_functions_extern` that declares `lmlang_runtime_error` as an external symbol without emitting the function body, used by per-function modules.
- **BFS dirty propagation:** When a function changes, all its transitive callers are marked as dirty dependents via BFS through the reverse call graph. This is correct but conservative (a callee signature change requires caller recompilation).
- **Lazy IncrementalState:** The ProgramService creates IncrementalState on first incremental compile, avoiding unnecessary state for programs that never compile.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Duplicate symbol conflicts in per-function .o linking**
- **Found during:** Task 1 (incremental compilation integration tests)
- **Issue:** Each per-function module emitted its own `lmlang_runtime_error` function body, causing duplicate symbol errors at link time
- **Fix:** Created `declare_runtime_functions_extern` for extern-only declarations in per-function modules; emitted the runtime body once in a dedicated `runtime.o`
- **Files modified:** `crates/lmlang-codegen/src/runtime.rs`, `crates/lmlang-codegen/src/compiler.rs`
- **Verification:** All 4 integration tests pass with multi-object linking
- **Committed in:** f703126 (Task 1 commit)

**2. [Rule 3 - Blocking] Entry function name conflict in separate modules**
- **Found during:** Task 1 (incremental compilation integration tests)
- **Issue:** The entry function "main" in its per-function .o file conflicted with the `main` wrapper function. The wrapper called `__lmlang_main` but the per-function module exported it as "main"
- **Fix:** Added rename of entry function to `__lmlang_main` in per-function module before object emission
- **Files modified:** `crates/lmlang-codegen/src/compiler.rs`
- **Verification:** Entry function correctly called through main wrapper
- **Committed in:** f703126 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking issues)
**Impact on plan:** Both fixes were necessary for per-function object file architecture to work. No scope creep.

## Issues Encountered

- The plan referenced `crates/lmlang-codegen/src/codegen.rs` in files_modified but no changes were needed there since the existing `compile_function` API was sufficient for per-function compilation.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Incremental compilation engine is complete and tested
- Dirty status API endpoint available for agent tooling
- Per-function .o architecture enables future parallel compilation optimizations
- Plan 04 (combined verification/compilation pipeline) can build on this infrastructure

---
*Phase: 06-full-contract-system-incremental-compilation*
*Completed: 2026-02-18*

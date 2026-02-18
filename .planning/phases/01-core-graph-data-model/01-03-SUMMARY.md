---
phase: 01-core-graph-data-model
plan: 03
subsystem: core
tags: [rust, function-def, closure, module-tree, capture, visibility]

# Dependency graph
requires:
  - phase: 01-core-graph-data-model (plan 01)
    provides: TypeId, FunctionId, ModuleId, Visibility
  - phase: 01-core-graph-data-model (plan 02)
    provides: SemanticNode with ModuleDef stub, ComputeNode, node.rs
provides:
  - FunctionDef with typed parameters, return type, closure captures, nesting
  - CaptureMode enum (ByValue, ByRef, ByMutRef)
  - ModuleDef (canonical, replaces stub) with id, name, parent, visibility
  - ModuleTree with hierarchical parent/child tracking, function and type_def registration
affects: [01-core-graph-data-model plan 04 (ProgramGraph container)]

# Tech tracking
tech-stack:
  added: []
  patterns: [closure-as-function-with-captures, module-tree-with-path, stub-refactor-to-canonical]

key-files:
  created:
    - crates/lmlang-core/src/function.rs
    - crates/lmlang-core/src/module.rs
  modified:
    - crates/lmlang-core/src/node.rs
    - crates/lmlang-core/src/lib.rs

key-decisions:
  - "Closures are FunctionDefs with is_closure=true and non-empty captures -- no separate closure type"
  - "ModuleDef gains id field (ModuleId) compared to the Plan 02 stub which only had name/parent/visibility"
  - "ModuleTree serde roundtrip uses structural comparison due to HashMap non-deterministic key ordering"

patterns-established:
  - "Closure pattern: closures are functions with captures, not a separate type"
  - "Module tree pattern: HashMap-based tree with parent/child/functions/type_defs tracking"
  - "Stub-to-canonical refactor: temporary stubs marked with TODO(plan-N) get replaced by canonical definitions"

requirements-completed: [GRAPH-04]

# Metrics
duration: 3min
completed: 2026-02-18
---

# Phase 1 Plan 3: Function & Module Definitions Summary

**FunctionDef with typed params, closure captures (3 modes), nesting support, plus ModuleTree with hierarchical parent/child and per-module function/type registration**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-18T05:54:12Z
- **Completed:** 2026-02-18T05:57:35Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- FunctionDef with full closure support: typed parameters, return type, entry_node, captures with ByValue/ByRef/ByMutRef modes, nesting via parent_function
- ModuleTree with hierarchical module management: add_module, add_function, add_type_def, path computation, children lookup
- Refactored temporary ModuleDef stub from node.rs into canonical module.rs definition (completing the TODO(plan-03) from Plan 02)
- All types properly derive Serialize/Deserialize for persistence

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement function definitions with closure support** - `5d721d6` (feat)
2. **Task 2: Implement module tree and refactor ModuleDef** - `d06fbfd` (feat)

## Files Created/Modified
- `crates/lmlang-core/src/function.rs` - FunctionDef, Capture, CaptureMode with constructors and convenience methods
- `crates/lmlang-core/src/module.rs` - ModuleDef, ModuleTree with hierarchical management
- `crates/lmlang-core/src/node.rs` - Removed ModuleDef stub, imports from module.rs, updated tests for new id field
- `crates/lmlang-core/src/lib.rs` - Added function/module module declarations and re-exports

## Decisions Made
- Closures are FunctionDefs with is_closure=true and non-empty captures -- no separate closure type needed
- ModuleDef now includes an `id: ModuleId` field (the Plan 02 stub only had name/parent/visibility)
- Closure constructor defaults visibility to Private (closures are typically internal)
- ModuleTree serde roundtrip test uses structural comparison rather than JSON string comparison due to HashMap non-deterministic key ordering

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed ModuleTree serde roundtrip test**
- **Found during:** Task 2
- **Issue:** HashMap key ordering is non-deterministic in serde_json, so comparing serialized JSON strings fails intermittently
- **Fix:** Changed test to compare structural values (root_id, module names, children, functions, paths) instead of raw JSON strings
- **Files modified:** crates/lmlang-core/src/module.rs
- **Verification:** Test passes consistently
- **Committed in:** d06fbfd (part of Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Necessary fix for test correctness. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- FunctionDef and ModuleTree are ready for Plan 04's ProgramGraph container
- ProgramGraph will use ModuleTree for module hierarchy and store FunctionDefs in a lookup table
- All types serialize/deserialize correctly for graph persistence

---
*Phase: 01-core-graph-data-model*
*Completed: 2026-02-18*

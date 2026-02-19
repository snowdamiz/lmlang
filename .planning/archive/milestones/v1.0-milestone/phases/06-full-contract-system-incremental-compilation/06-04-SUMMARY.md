---
phase: 06-full-contract-system-incremental-compilation
plan: 04
subsystem: contracts
tags: [invariant, module-boundary, mini-evaluator, contract-checking]

# Dependency graph
requires:
  - phase: 06-full-contract-system-incremental-compilation
    provides: "Contract ops (Precondition/Postcondition/Invariant), inline contract evaluation in interpreter, check_invariants_for_value skeleton"
provides:
  - "Mini-subgraph evaluator for on-the-fly invariant condition checking at module boundaries"
  - "Module-boundary invariant enforcement wired into interpreter Call handler"
  - "CNTR-02/03/04 requirements fully satisfied and tracked"
affects: [phase-07, phase-08]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Mini-subgraph evaluation: recursive on-the-fly node evaluation for contract checking outside worklist context"
    - "Module-boundary gating: cross-module calls trigger invariant checks before frame push"

key-files:
  created: []
  modified:
    - "crates/lmlang-check/src/contracts/check.rs"
    - "crates/lmlang-check/src/interpreter/state.rs"
    - ".planning/REQUIREMENTS.md"

key-decisions:
  - "Mini-subgraph evaluation walks backward from contract node, substitutes Parameter with arg_value, evaluates Const to literal, and forwards through eval_op for arithmetic/comparison"
  - "check_invariants_for_value no longer accepts node_values parameter since it self-evaluates via mini-subgraph evaluator"
  - "Evaluation errors in invariant subgraph treated as conservative violations (report rather than silently ignore)"

patterns-established:
  - "Mini-evaluator pattern: evaluate_subgraph_node recursively resolves node dependencies with local_values cache, enabling contract checking outside normal worklist execution"

requirements-completed: [CNTR-04]

# Metrics
duration: 4min
completed: 2026-02-19
---

# Phase 6 Plan 4: Module-Boundary Invariant Checking Summary

**Mini-subgraph evaluator for on-the-fly invariant checking at cross-module call boundaries, closing the CNTR-04 gap**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-19T02:30:22Z
- **Completed:** 2026-02-19T02:34:27Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Implemented `evaluate_invariant_for_value` with recursive mini-subgraph evaluation that substitutes Parameter nodes with concrete values and evaluates condition subgraphs on-the-fly
- Wired module-boundary invariant checking into interpreter's Call handler: cross-module calls now trigger invariant checks before frame push
- Updated check_invariants_for_value to use mini-evaluator (removed broken node_values dependency)
- Updated REQUIREMENTS.md to reflect CNTR-02, CNTR-03, CNTR-04 as complete
- All 373 workspace tests pass including 4 new tests

## Task Commits

Each task was committed atomically:

1. **Task 1: Add evaluate_invariant_for_value with mini-subgraph evaluation, wire into Call handler** - `dd741e0` (feat)
2. **Task 2: Update REQUIREMENTS.md for completed contract requirements** - `6cd9189` (docs)

## Files Created/Modified
- `crates/lmlang-check/src/contracts/check.rs` - Added evaluate_invariant_for_value, evaluate_subgraph_node, updated check_invariants_for_value, added 3 unit tests
- `crates/lmlang-check/src/interpreter/state.rs` - Added module-boundary invariant checking in EvalResult::Call handler, added cross-module integration test
- `.planning/REQUIREMENTS.md` - Marked CNTR-02/03/04 as complete in checkboxes and traceability table

## Decisions Made
- Mini-subgraph evaluator uses recursive backward walk + forward evaluation with local HashMap cache, reusing existing eval_op for arithmetic/comparison nodes
- Removed node_values parameter from check_invariants_for_value entirely since the new approach self-evaluates invariant subgraphs
- Evaluation errors treated as conservative violations rather than silent no-ops (the previous behavior)
- Same-module calls skip the boundary check entirely (zero overhead for internal calls)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Phase 6 is now fully complete: all 4 plans executed, all contract requirements satisfied
- All 373 tests pass across the workspace
- CNTR-01 through CNTR-05 are all marked Complete in REQUIREMENTS.md
- Ready for Phase 7 (Multi-Agent Concurrency)

## Self-Check: PASSED

- FOUND: crates/lmlang-check/src/contracts/check.rs
- FOUND: crates/lmlang-check/src/interpreter/state.rs
- FOUND: .planning/REQUIREMENTS.md
- FOUND: 06-04-SUMMARY.md
- FOUND: dd741e0 (Task 1 commit)
- FOUND: 6cd9189 (Task 2 commit)

---
*Phase: 06-full-contract-system-incremental-compilation*
*Completed: 2026-02-19*

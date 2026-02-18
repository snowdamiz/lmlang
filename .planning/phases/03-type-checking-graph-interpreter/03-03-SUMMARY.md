---
phase: 03-type-checking-graph-interpreter
plan: 03
subsystem: interpreter
tags: [loop-op, back-edge, re-evaluation, work-list, memory-based-loop, requirements-tracking]

# Dependency graph
requires:
  - phase: 03-type-checking-graph-interpreter/02
    provides: Interpreter with work-list evaluation, propagate_control_flow for Loop/Branch/IfElse, control-gated scheduling
provides:
  - Loop back-edge re-evaluation in propagate_control_flow (state.rs)
  - Real ComputeOp::Loop integration test with memory-based iteration and back-edge control flow
  - EXEC-01 requirement marked Complete
affects: [04-parser-frontend]

# Tech tracking
tech-stack:
  added: []
  patterns: [loop body state reset with external readiness pre-credit, memory-based loop variables via Alloc/Store/Load]

key-files:
  created: []
  modified:
    - crates/lmlang-check/src/interpreter/state.rs
    - crates/lmlang-check/src/interpreter/mod.rs
    - .planning/REQUIREMENTS.md
    - .planning/phases/03-type-checking-graph-interpreter/03-02-SUMMARY.md

key-decisions:
  - "Memory-based loop variables (Alloc/Store/Load) instead of Phi for loop-carried values -- avoids circular Phi<->Loop dependency in work-list model"
  - "BFS loop body discovery: follow all outgoing edges from branch-0 control successors, stopping at Loop node, to find nodes that need re-evaluation"
  - "External readiness pre-credit: body nodes with data inputs from outside the loop start with readiness pre-set to count of available external sources"
  - "Control back-edge pattern: store_i_body -> control -> load_i_hdr creates re-evaluation chain through condition into Loop"

patterns-established:
  - "Loop body reset: propagate_control_flow clears evaluated/values/readiness/control_ready for BFS-discovered body nodes, pre-credits external data inputs, re-applies control_ready for activated successors"
  - "Memory-based loops: Alloc/Store/Load for loop variables with control edges for ordering (Store -> Load back-edge triggers condition re-evaluation)"

requirements-completed: [EXEC-01]

# Metrics
duration: 11min
completed: 2026-02-18
---

# Phase 3 Plan 3: Verification Gap Closure Summary

**Loop back-edge re-evaluation with memory-based iteration exercising ComputeOp::Loop end-to-end, plus EXEC-01 requirements tracking update**

## Performance

- **Duration:** 11 min
- **Started:** 2026-02-18T20:15:58Z
- **Completed:** 2026-02-18T20:27:11Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Loop op now works end-to-end with actual back-edge iteration: sum_loop(5) = 15, sum_loop(1) = 1, sum_loop(0) = 0
- Loop body state reset in propagate_control_flow enables re-evaluation of body nodes on each iteration while preserving external data readiness
- EXEC-01 marked Complete in REQUIREMENTS.md and 03-02-SUMMARY.md
- 115 total tests pass (112 existing + 3 new, zero regressions)

## Task Commits

Each task was committed atomically:

1. **Task 1: Fix Loop back-edge re-evaluation and add real Loop integration test** - `86244c1` (feat)
2. **Task 2: Update EXEC-01 requirements tracking** - `968c3be` (docs)

## Files Created/Modified
- `crates/lmlang-check/src/interpreter/state.rs` - Added loop body reset logic in propagate_control_flow: BFS body discovery, state clearing with external readiness pre-credit, control_ready re-application
- `crates/lmlang-check/src/interpreter/mod.rs` - Added build_loop_sum_graph helper and 3 integration tests (integration_loop_with_real_loop_op, _n1, _n0)
- `.planning/REQUIREMENTS.md` - EXEC-01 checkbox and traceability table updated to Complete
- `.planning/phases/03-type-checking-graph-interpreter/03-02-SUMMARY.md` - requirements-completed updated to [EXEC-01]

## Decisions Made
- **Memory-based loop variables:** Used Alloc/Store/Load instead of Phi nodes for loop-carried values. Phi nodes in the work-list model create a circular dependency (Phi needs Loop control to select port, Loop needs data through Phi for condition). Memory-based variables break this cycle: body stores update memory, back-edge control triggers condition re-read from memory, condition feeds Loop naturally.
- **BFS body discovery via all edge types:** The body reset BFS follows both control and data outgoing edges from branch-0 successors. This naturally discovers the condition re-evaluation path (store_i_body -> load_i_hdr -> cond) as part of the loop body, ensuring it gets reset and re-evaluated each iteration.
- **External readiness pre-credit:** When resetting body nodes, data inputs from nodes outside the body (e.g., Alloc addresses, constants, parameters) still have their values available but won't re-fire readiness. Pre-crediting readiness to the count of available external sources prevents deadlock.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Graph design: Phi-based loop creates circular dependency**
- **Found during:** Task 1 (initial loop graph design)
- **Issue:** Phi nodes control-gated by Loop + Loop needing data through Phi creates unresolvable work-list deadlock
- **Fix:** Switched to memory-based loop variables (Alloc/Store/Load) which naturally work with the imperative work-list model
- **Files modified:** crates/lmlang-check/src/interpreter/mod.rs
- **Verification:** All 3 loop tests pass with memory-based approach

**2. [Rule 1 - Bug] State reset cleared control_ready for activated successors**
- **Found during:** Task 1 (debugging loop body scheduling)
- **Issue:** BFS discovered activated successors as body nodes, then cleared their control_ready, preventing them from being scheduled
- **Fix:** Re-apply control_ready for activated successors after the body reset
- **Files modified:** crates/lmlang-check/src/interpreter/state.rs
- **Verification:** Loop body correctly schedules after state reset

**3. [Rule 1 - Bug] External data edges not pre-credited after readiness reset**
- **Found during:** Task 1 (debugging load node readiness)
- **Issue:** Load nodes in loop body have data edges from Alloc (outside body). After reset, readiness=0 but Alloc won't re-fire, causing deadlock
- **Fix:** Pre-compute external readiness count for each body node and set readiness to that count during reset
- **Files modified:** crates/lmlang-check/src/interpreter/state.rs
- **Verification:** Load nodes correctly become data-ready on re-evaluation

---

**Total deviations:** 3 auto-fixed (3 bugs discovered during loop implementation)
**Impact on plan:** All fixes essential for correct loop iteration. The memory-based approach is documented as a deliberate design choice matching the imperative work-list execution model.

## Issues Encountered
None beyond the auto-fixed bugs above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 3 verification gaps fully closed
- All ComputeOp variants now have end-to-end test coverage including Loop with real iteration
- 115 tests provide comprehensive regression safety for Phase 4 development
- EXEC-01 and CNTR-01 both marked Complete

## Self-Check: PASSED
- All 5 files verified present
- Both task commits (86244c1, 968c3be) verified in git log
- integration_loop_with_real_loop_op test present (3 occurrences)
- evaluated.remove present in state.rs (2 occurrences)
- EXEC-01 marked Complete in REQUIREMENTS.md
- 115 tests pass (0 failures)

---
*Phase: 03-type-checking-graph-interpreter*
*Completed: 2026-02-18*

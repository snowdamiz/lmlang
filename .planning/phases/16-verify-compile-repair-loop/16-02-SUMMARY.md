---
phase: 16-verify-compile-repair-loop
plan: 02
subsystem: api
tags: [autonomy, planner, diagnostics, repair-loop, dashboard]

requires:
  - phase: 16-verify-compile-repair-loop
    provides: diagnostics-rich action and attempt evidence persisted per execution attempt
provides:
  - retry planner prompts with deterministic diagnostics context blocks
  - targeted repair retry notes and explicit terminal stop-reason detail for non-retryable/unsafe exits
  - diagnostics metadata projection on agent/dashboard execution views
affects: [phase-16-hardening, phase-17-benchmarks, operator-observability]

tech-stack:
  added: []
  patterns: [diagnostics-aware planner retries, targeted repair transition notes, optional execution diagnostics projection]

key-files:
  created: []
  modified:
    - crates/lmlang-server/src/autonomy_planner.rs
    - crates/lmlang-server/src/autonomous_runner.rs
    - crates/lmlang-server/src/project_agent.rs
    - crates/lmlang-server/src/schema/agent_control.rs
    - crates/lmlang-server/src/schema/dashboard.rs
    - crates/lmlang-server/src/handlers/agent_control.rs
    - crates/lmlang-server/src/handlers/dashboard.rs

key-decisions:
  - "Planner prompt payloads now include `Latest execution diagnostics` only for retry attempts to keep first-attempt prompts minimal."
  - "Repair context is derived from session attempt evidence (`execution_attempts`) instead of transcript heuristics."
  - "Operator-facing diagnostics fields remain optional on existing response contracts for backward compatibility."

patterns-established:
  - "Retry prompt pattern: deterministic diagnostics JSON block + explicit targeted-repair instruction"
  - "Projection pattern: action-level diagnostics and attempt-level diagnostics summary surfaced together"

requirements-completed: [AUT-07, AUT-08]

duration: 9 min
completed: 2026-02-19
---

# Phase 16 Plan 02: Targeted Repair Loop Integration Summary

**Retry attempts now carry structured verify/compile diagnostics into planner prompts and expose compact diagnostics metadata through agent/dashboard execution views.**

## Performance

- **Duration:** 9 min
- **Started:** 2026-02-19T18:36:00Z
- **Completed:** 2026-02-19T18:45:24Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments

- Extended planner contract path with `PlannerRepairContext` and retry-only `Latest execution diagnostics` prompt rendering.
- Updated autonomous runner to build repair context from latest diagnostics-rich attempt evidence and produce targeted retry notes.
- Preserved explicit terminal stop detail for planner rejection and retry-budget exhaustion paths.
- Added session helper to retrieve latest diagnostics context from `execution_attempts`.
- Added diagnostics projection schema/view mapping for agent control and dashboard chat responses.

## Task Commits

1. **Task 1: Extend planner prompt path with structured repair diagnostics context** - `1417bbb` (`feat`)
2. **Task 2: Implement targeted repair transition behavior and surface diagnostics metadata** - `1554cab` (`feat`)

**Plan metadata:** recorded in follow-up docs commit for this plan.

## Files Created/Modified

- `crates/lmlang-server/src/autonomy_planner.rs` - Added retry diagnostics context type and prompt assembly behavior with focused tests.
- `crates/lmlang-server/src/autonomous_runner.rs` - Wired diagnostics context into planner calls and refined retry/terminal notes/details.
- `crates/lmlang-server/src/project_agent.rs` - Added helper for latest diagnostics-rich attempt context retrieval.
- `crates/lmlang-server/src/schema/agent_control.rs` - Added diagnostics view schema fields for action/attempt execution metadata.
- `crates/lmlang-server/src/schema/dashboard.rs` - Added optional dashboard diagnostics projection field.
- `crates/lmlang-server/src/handlers/agent_control.rs` - Mapped action/attempt diagnostics into API response views.
- `crates/lmlang-server/src/handlers/dashboard.rs` - Surfaced diagnostics summary in dashboard chat response payload.

## Verification

- `cargo test -p lmlang-server`

## Decisions Made

- Retried planning receives deterministic diagnostic context from the latest failed action (kind/class/retryability/key diagnostics/attempt position).
- Agent/dashboard execution projections expose diagnostics summaries without forcing clients to consume full raw logs.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None.

## Next Phase Readiness

- Phase 16-03 can focus on hardening/integration assertions and operator documentation updates.
- AUT-07/AUT-08 targeted repair loop behavior is now integrated and observable.

---
*Phase: 16-verify-compile-repair-loop*
*Completed: 2026-02-19*

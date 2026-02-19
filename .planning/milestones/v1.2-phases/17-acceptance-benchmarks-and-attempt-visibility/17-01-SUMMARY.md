---
phase: 17-acceptance-benchmarks-and-attempt-visibility
plan: 01
subsystem: api
tags: [autonomy, timeline, dashboard, agent-control]
requires:
  - phase: 16-verify-compile-repair-loop
    provides: diagnostics-aware retry attempts and latest execution metadata projection
provides:
  - bounded execution attempt history projection helpers
  - agent/session/chat schema support for `execution_attempts`
  - dashboard chat payload support for `execution_attempts`
affects: [phase17-benchmarks, dashboard-ui, operator-observability]
tech-stack:
  added: []
  patterns: [bounded timeline projection, backward-compatible optional response fields]
key-files:
  created: []
  modified:
    - crates/lmlang-server/src/schema/autonomy_execution.rs
    - crates/lmlang-server/src/schema/agent_control.rs
    - crates/lmlang-server/src/schema/dashboard.rs
    - crates/lmlang-server/src/handlers/agent_control.rs
    - crates/lmlang-server/src/handlers/dashboard.rs
    - crates/lmlang-server/tests/integration_test.rs
key-decisions:
  - "Expose full attempt history via optional `execution_attempts` while preserving existing `execution` latest-summary payloads."
  - "Use deterministic bounded history projection (latest 8 attempts) for operator-facing timeline rendering safety."
patterns-established:
  - "Timeline contract: `execution` remains latest-attempt compatibility layer; `execution_attempts` carries ordered attempt rows."
  - "Attempt/action diagnostics are sourced from persisted `session.execution_attempts`, never transcript parsing."
requirements-completed: [AUT-09]
duration: 28 min
completed: 2026-02-19
---

# Phase 17 Plan 01: Attempt Visibility Contract Summary

**Agent and dashboard APIs now expose deterministic structured autonomous attempt history through backward-compatible `execution_attempts` timeline fields.**

## Performance

- **Duration:** 28 min
- **Started:** 2026-02-19T18:35:00Z
- **Completed:** 2026-02-19T19:03:00Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

- Added bounded attempt-history helpers and diagnostics/stop-reason projection helpers in autonomy execution schema.
- Extended agent control and dashboard response contracts with optional `execution_attempts` fields.
- Wired agent and dashboard handlers to project timeline rows directly from persisted `execution_attempts` state.
- Added integration coverage proving timeline fields are surfaced in both agent detail and dashboard chat responses.

## Task Commits

1. **Task 1: Add structured attempt-history projection models and mapping helpers** - `7601e3f` (feat)
2. **Task 2: Wire timeline visibility through project-agent and dashboard handlers** - `a7b51da` (feat)

**Plan metadata:** pending docs closeout commit for plan artifacts.

## Files Created/Modified

- `crates/lmlang-server/src/schema/autonomy_execution.rs` - Added bounded timeline and attempt helper methods.
- `crates/lmlang-server/src/schema/agent_control.rs` - Added `execution_attempts` fields for session/chat responses.
- `crates/lmlang-server/src/schema/dashboard.rs` - Added `execution_attempts` on dashboard AI chat responses.
- `crates/lmlang-server/src/handlers/agent_control.rs` - Added deterministic attempt timeline projection helpers.
- `crates/lmlang-server/src/handlers/dashboard.rs` - Reused shared projection for dashboard chat responses.
- `crates/lmlang-server/tests/integration_test.rs` - Added phase17 contract test for attempt timeline visibility.

## Decisions Made

- Kept `execution` untouched for existing consumers; introduced additive `execution_attempts` for richer timeline UI/testing use-cases.
- Centralized attempt projection in handler helpers to guarantee deterministic ordering and bounded payload size.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Backend timeline contract is complete and ready for benchmark-oriented acceptance assertions.
- Plan 17-02 can assert benchmark intent markers from structured action summaries without schema churn.

---
*Phase: 17-acceptance-benchmarks-and-attempt-visibility*
*Completed: 2026-02-19*

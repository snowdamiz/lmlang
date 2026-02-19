---
phase: 14-action-protocol-and-planner-contract
plan: 02
subsystem: api
tags: [autonomy, planner, routing, handlers, dashboard]

requires:
  - phase: 14-action-protocol-and-planner-contract
    provides: Versioned planner schema and semantic validation from 14-01
provides:
  - Planner runtime adapter wired into non-command chat routing
  - Structured planner success/failure metadata in agent and dashboard responses
  - Autonomous runner planner integration for non-hello-world goals
affects: [phase-14-verification, phase-15-executor]

tech-stack:
  added: []
  patterns:
    - planner-first-non-command-routing
    - structured-outcome-projection-in-response-schema

key-files:
  created:
    - crates/lmlang-server/src/autonomy_planner.rs
  modified:
    - crates/lmlang-server/src/llm_provider.rs
    - crates/lmlang-server/src/handlers/agent_control.rs
    - crates/lmlang-server/src/handlers/dashboard.rs
    - crates/lmlang-server/src/autonomous_runner.rs
    - crates/lmlang-server/src/schema/agent_control.rs
    - crates/lmlang-server/src/schema/dashboard.rs

key-decisions:
  - "Non-command prompts now always enter planner path; no plain external-chat fallback is used for execution intent."
  - "Planner provider/parse/validation failures are normalized into structured failed outcomes rather than API transport errors."
  - "Autonomous runner records planner outcomes for non-command goals and settles run state instead of attempting unsafe unstructured execution."

patterns-established:
  - "Route split: deterministic command fast-path first, planner contract path for everything else."
  - "Response payloads expose planner outcome envelope (status, actions, failure, validation detail) for operator transparency."

requirements-completed: [AUT-01, AUT-02, AUT-03]

duration: 18 min
completed: 2026-02-19
---

# Phase 14 Plan 02: Action Protocol and Planner Contract Summary

**Planner runtime now governs non-command chat/autonomy routing with typed acceptance/failure outputs and response-level planner metadata.**

## Performance

- **Duration:** 18 min
- **Started:** 2026-02-19T17:26:00Z
- **Completed:** 2026-02-19T17:34:13Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments

- Added `autonomy_planner` runtime adapter that builds planner prompts, parses JSON contract output, applies semantic validation, and emits typed accepted/rejected outcomes.
- Extended provider helper to support JSON-mode chat requests while preserving plain chat behavior for existing command flows.
- Routed non-command prompts in both `/programs/{id}/agents/{agent_id}/chat` and `/dashboard/ai/chat` through planner contract path and surfaced structured planner metadata in response payloads.
- Updated autonomous runner to consume planner outcomes for non-hello-world goals while preserving deterministic hello-world command behavior.

## Task Commits

Each task was committed atomically:

1. **Task 1: Build planner runtime adapter with strict validation handoff** - `f0cdd90` (feat)
2. **Task 2: Route non-command prompts through planner path in handlers** - `8d85728` (feat)

## Files Created/Modified

- `crates/lmlang-server/src/autonomy_planner.rs` - Planner runtime types, prompt construction, contract parse/validate flow, and unit tests.
- `crates/lmlang-server/src/llm_provider.rs` - JSON-mode chat helper for planner calls (`response_format: json_object`).
- `crates/lmlang-server/src/handlers/agent_control.rs` - Planner routing + structured outcome projection for project-agent chat.
- `crates/lmlang-server/src/handlers/dashboard.rs` - Dashboard AI chat planner payload passthrough.
- `crates/lmlang-server/src/autonomous_runner.rs` - Planner-driven non-command decision path.
- `crates/lmlang-server/src/schema/agent_control.rs` - Planner response metadata schema fields.
- `crates/lmlang-server/src/schema/dashboard.rs` - Dashboard planner response metadata schema field.

## Decisions Made

- Removed non-command plain-chat fallback from handler routing to satisfy AUT-01 contract-first behavior.
- Standardized planner failures as structured payloads (code/message/retryable + validation details) to support deterministic downstream handling.
- Kept explicit hello-world command path untouched for backward compatibility while planner executor remains in future phases.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- Full integration suite still contains unrelated pre-existing dashboard shell assertions and intermittent compile/run contention when run fully in parallel. Targeted phase10 compatibility checks passed for command-path and autonomous hello-world flows.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Planner contract routing and response plumbing are ready for hardening + end-to-end phase14 integration tests in `14-03`.
- Next plan can validate success/failure contract behavior through deterministic test fixtures.

---
*Phase: 14-action-protocol-and-planner-contract*
*Completed: 2026-02-19*

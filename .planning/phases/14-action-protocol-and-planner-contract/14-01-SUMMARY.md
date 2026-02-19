---
phase: 14-action-protocol-and-planner-contract
plan: 01
subsystem: api
tags: [autonomy, planner, schema, validation, serde]

requires: []
provides:
  - Versioned autonomy planner envelope and ordered action schema
  - Semantic validation with machine-readable error codes for planner payloads
  - Operator API documentation for planner contract success/failure semantics
affects: [autonomous-runner, agent-control, dashboard-chat, planner-runtime]

tech-stack:
  added: []
  patterns:
    - strict-serde-contracts
    - explicit-semantic-validation-before-execution

key-files:
  created:
    - crates/lmlang-server/src/schema/autonomy_plan.rs
  modified:
    - crates/lmlang-server/src/schema/mod.rs
    - crates/lmlang-server/src/lib.rs
    - docs/api/operator-endpoints.md

key-decisions:
  - "Planner contract version uses explicit date-based identifier 2026-02-19 for deterministic compatibility checks."
  - "Action payload structs keep optional fields where semantic validation must return structured missing-field reasons instead of parse-only failures."
  - "Validation enforces action-level guardrails (count limits, required fields, allowed values) before any execution routing."

patterns-established:
  - "Planner contracts use deny_unknown_fields + semantic validator pass for robust server-side safety."
  - "Validation errors are machine-readable with stable codes and optional action index + field path metadata."

requirements-completed: [AUT-02, AUT-03]

duration: 20 min
completed: 2026-02-19
---

# Phase 14 Plan 01: Action Protocol and Planner Contract Summary

**Versioned planner action contract with semantic validation and structured failure semantics for deterministic autonomous routing.**

## Performance

- **Duration:** 20 min
- **Started:** 2026-02-19T17:06:00Z
- **Completed:** 2026-02-19T17:25:59Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Added `autonomy_plan` as a first-class schema module with a versioned envelope, ordered action variants, metadata, and optional structured failure payload.
- Implemented semantic validation across envelope and action payloads with explicit machine-readable reason codes for unsupported version, malformed payloads, and missing required fields.
- Documented planner contract version policy and failure/validation semantics in operator endpoint docs.

## Task Commits

Each task was committed atomically:

1. **Task 1: Create versioned planner schema envelope and action model** - `2b46344` (feat)
2. **Task 2: Implement semantic validation and document contract rules** - `ae468db` (feat)

## Files Created/Modified

- `crates/lmlang-server/src/schema/autonomy_plan.rs` - New planner contract schema, action payload models, conversion helpers, semantic validator, and unit tests.
- `crates/lmlang-server/src/schema/mod.rs` - Exported the new planner schema module.
- `crates/lmlang-server/src/lib.rs` - Re-exported planner schema surface at crate root.
- `docs/api/operator-endpoints.md` - Added planner contract section with success/failure examples and validation code semantics.

## Decisions Made

- Adopted a date-stamped contract version string to keep compatibility checks explicit and auditable.
- Chose semantic validation over parse-only strictness for required action fields so handlers receive structured reason codes and field-level context.
- Enforced bounded action and payload sizes at schema-validation layer to prevent unsafe/oversized plans before execution.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- `cargo test --package lmlang-server` currently fails two pre-existing dashboard integration assertions (`phase10_dashboard_routes_serve_shell_and_assets`, `phase10_dashboard_and_observe_routes_coexist_with_reuse_contract`) unrelated to planner schema work. New schema test suite passed (`cargo test --package lmlang-server schema::autonomy_plan::tests`).

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Planner contract schema and validation gate are now ready for runtime integration in `14-02`.
- The next plan can route non-command prompts through typed planner output instead of plain chat text.

---
*Phase: 14-action-protocol-and-planner-contract*
*Completed: 2026-02-19*

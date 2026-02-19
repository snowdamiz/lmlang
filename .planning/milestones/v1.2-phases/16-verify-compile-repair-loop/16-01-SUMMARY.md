---
phase: 16-verify-compile-repair-loop
plan: 01
subsystem: api
tags: [autonomy, diagnostics, verify, compile, retry-loop]

requires:
  - phase: 15-generic-graph-build-executor
    provides: bounded autonomous loop with typed action outcomes and stop reasons
provides:
  - normalized diagnostics payloads on action and error evidence rows
  - deterministic verify/compile failure summaries with retryability metadata
  - session-persisted diagnostics context for downstream repair planning
affects: [phase-16-targeted-repair, phase-17-benchmark-visibility, operator-endpoints]

tech-stack:
  added: []
  patterns: [diagnostics normalization, typed attempt evidence persistence, backward-compatible schema extension]

key-files:
  created: []
  modified:
    - crates/lmlang-server/src/schema/autonomy_execution.rs
    - crates/lmlang-server/src/autonomy_executor.rs
    - crates/lmlang-server/src/autonomous_runner.rs
    - crates/lmlang-server/src/project_agent.rs

key-decisions:
  - "Diagnostics metadata is carried at both action and error levels to preserve retry context during downstream projections."
  - "Verify and compile failures emit stable diagnostics classes (`verify_failure`, `compile_failure`) for deterministic repair routing."
  - "Diagnostics fields remain optional to preserve backward compatibility for clients ignoring new evidence metadata."

patterns-established:
  - "Action evidence pattern: failed rows carry structured diagnostics plus compact message samples"
  - "Session persistence pattern: append attempt snapshots with diagnostics intact before terminal outcome projection"

requirements-completed: [AUT-07, AUT-08]

duration: 34 min
completed: 2026-02-19
---

# Phase 16 Plan 01: Diagnostics Capture Foundation Summary

**Autonomous execution now persists normalized verify/compile diagnostics as machine-readable evidence suitable for targeted repair planning.**

## Performance

- **Duration:** 34 min
- **Started:** 2026-02-19T18:02:00Z
- **Completed:** 2026-02-19T18:36:57Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Extended execution schema with optional structured diagnostics metadata on both action rows and classified errors.
- Added deterministic diagnostics emission in executor failure paths, including verify-specific summaries and compile retryability projection.
- Updated runner verify-gate failure handling to preserve normalized diagnostics inside attempt evidence.
- Added persistence coverage in project-agent tests to prove verify and compile diagnostics survive attempt and outcome storage.

## Task Commits

1. **Task 1: Normalize and persist verify/compile diagnostics in execution evidence** - `aee09d4` (`feat`)
2. **Task 2: Carry diagnostics-rich attempt evidence through runner/session state** - `bb6c286` (`feat`)

**Plan metadata:** recorded in follow-up docs commit for this plan.

## Files Created/Modified

- `crates/lmlang-server/src/schema/autonomy_execution.rs` - Added diagnostics schema types and optional diagnostics fields on action/error payloads.
- `crates/lmlang-server/src/autonomy_executor.rs` - Normalized verify/compile diagnostics emission and deterministic helper logic with unit tests.
- `crates/lmlang-server/src/autonomous_runner.rs` - Preserved verify-gate diagnostics in failed attempt action rows.
- `crates/lmlang-server/src/project_agent.rs` - Added tests validating diagnostics persistence through session attempt/outcome storage.

## Decisions Made

- Diagnostics are emitted as compact summaries plus bounded message samples to keep payloads deterministic and concise.
- Retryability remains sourced from existing error classification and is mirrored into diagnostics metadata for repair logic consumers.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 16-02 can now consume per-attempt diagnostics context for targeted replanning.
- Existing phase15 stop-reason behavior remains stable while exposing richer optional evidence metadata.

---
*Phase: 16-verify-compile-repair-loop*
*Completed: 2026-02-19*

# Phase 17: Acceptance Benchmarks and Attempt Visibility - Research

**Researched:** 2026-02-19
**Domain:** Autonomous acceptance benchmarks, attempt-history observability, and operator timeline UX
**Confidence:** HIGH

## Summary

Phase 16 established diagnostics-aware retry behavior and compact latest-attempt metadata, but Phase 17 still needs explicit acceptance proof and richer visibility:

- AUT-10 and AUT-11 are still unverified at milestone level because benchmark prompts are not yet codified as deterministic acceptance scenarios.
- Session state persists `execution_attempts`, but API projections mainly expose latest-attempt summaries, not full timeline-grade attempt history.
- Dashboard chat UI currently renders transcript text only; operators cannot inspect structured attempt/action rows without querying raw endpoint payloads.
- Existing tests validate repair-loop behavior but do not yet prove benchmark prompt coverage for calculator + two additional non-trivial tasks through the same generic pipeline.

Phase 17 should close this by adding benchmark acceptance tests and first-class attempt timeline projections across APIs and dashboard surfaces.

## Requirements Mapping

| Requirement | Planning implication |
|-------------|----------------------|
| AUT-09 | Expose full autonomous attempt history (attempt metadata + action-level outputs + terminal status) in operator-facing timeline/history views |
| AUT-10 | Add deterministic acceptance scenario for `Create a simple calculator` proving real autonomous attempt with calculator-specific structure plus verify/compile attempt evidence |
| AUT-11 | Add at least two additional benchmark scenarios through the same planner/executor path with persisted attempt records and comparable assertions |

## Existing Architecture Signals

### 1. Durable attempt evidence already exists in session state

- `project_agent.rs` persists `execution_attempts` and final `execution` outcome per `(program, agent)` session.
- This is a strong foundation for AUT-09 without introducing new persistence infrastructure.

### 2. API projections are latest-attempt oriented

- `to_latest_execution_view` currently exports compact metadata for the latest attempt only.
- Operators can see terminal status and latest diagnostics but not complete per-run progression in structured form.

### 3. Runner transcript contains timeline-like notes but not structured timeline models

- `autonomous_runner.rs` appends messages such as `Execution attempt N/M recorded...`.
- These messages aid readability but are not schema-stable for automation, filtering, or dashboard rendering.

### 4. Integration harness already supports deterministic planner sequencing

- `integration_test.rs` includes mock planner server helpers and request capture.
- This makes benchmark acceptance scenarios deterministic and CI-friendly without live model dependencies.

### 5. Dashboard has orchestration chat but no dedicated attempt timeline panel

- `static/dashboard/app.js` currently renders transcript chat log and status badges.
- No dedicated view exists for attempt list, per-action outputs, diagnostics classes, or terminal stop detail history.

## Recommended Phase 17 Design

1. Add structured attempt-history projection contract:
   - Keep current `execution` latest summary for compatibility.
   - Add explicit history/timeline view shape with ordered attempts and per-action rows.
2. Harden execution evidence summaries for benchmark assertions:
   - Ensure mutation/action summaries expose enough structure markers (for example calculator-related function/mutation evidence) for acceptance checks.
3. Implement deterministic benchmark acceptance suite:
   - Calculator benchmark (`Create a simple calculator`).
   - String utility benchmark.
   - State-machine/workflow benchmark.
   - Assert all run through planner/executor path and persist attempt records.
4. Add dashboard timeline surface:
   - Render structured attempts/actions/diagnostics/final outcome without requiring transcript parsing.
5. Update operator docs:
   - Document benchmark acceptance expectations and timeline fields for triage workflows.

## Risks and Mitigations

- Risk: benchmark prompts pass technically but produce shallow plans.
  Mitigation: require structure-level assertions on action evidence (mutation targets, verify/compile presence).
- Risk: timeline payload growth makes chat responses heavy.
  Mitigation: keep compact latest summary + bounded attempt history projection with concise action rows.
- Risk: dashboard timeline UI diverges from API contract.
  Mitigation: add integration assertions on timeline fields and keep docs aligned with serialized schema.
- Risk: regressions in phase16 diagnostics projection.
  Mitigation: retain existing phase16 tests and add phase17 coverage as additive, not replacement.

## Testing Priorities

1. Deterministic calculator benchmark test proving verify/compile attempt and calculator-oriented mutation evidence.
2. Deterministic tests for two additional benchmark prompts via same generic autonomy path.
3. Assertions that attempt records persist across retries and terminal completion/failure states.
4. API contract tests for timeline/history projection fields.
5. Dashboard integration checks for timeline rendering path and terminal outcome visibility.

## Recommended Execution Order

1. **Plan 17-01:** Introduce structured attempt-history projection contract and API wiring (AUT-09 foundation).
2. **Plan 17-02:** Add benchmark acceptance scenarios (calculator + two additional prompts) with deterministic assertions (AUT-10/AUT-11 core).
3. **Plan 17-03:** Deliver dashboard timeline/history UX and operator docs/tests that surface benchmark attempt visibility end-to-end.

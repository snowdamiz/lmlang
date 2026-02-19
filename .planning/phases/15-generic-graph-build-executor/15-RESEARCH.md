# Phase 15: Generic Graph Build Executor - Research

**Researched:** 2026-02-19
**Domain:** Planner-action execution, bounded autonomy loop, and run-stop diagnostics
**Confidence:** HIGH

## Summary

Phase 14 established a strict planner contract and validated routing, but autonomous execution currently stops as soon as a plan is accepted. The runtime appends a transcript note and sets run status to idle with:

- `Autonomous planner accepted structured plan: ...`
- `Execution awaits generic planner action executor.`

Phase 15 should close this gap by executing validated planner actions directly against `ProgramService` primitives (mutate/verify/compile/simulate/history/query), then driving a bounded `plan -> apply -> verify -> replan` loop with explicit terminal reason codes.

## Requirements Mapping

| Requirement | Planning implication |
|-------------|----------------------|
| AUT-04 | Convert accepted planner actions into deterministic server-side operations against existing graph APIs |
| AUT-05 | Replace one-shot planner acceptance behavior with retry-bounded loop control and explicit replan transitions |
| AUT-06 | Persist machine-readable stop reasons and per-attempt evidence in transcript/API-visible structures |

## Existing Architecture Signals

### 1. Phase 14 already delivers typed plans

- `autonomy_planner.rs` returns `PlannerOutcome::Accepted` with validated `AutonomyPlanEnvelope`.
- `autonomy_plan.rs` already enforces action type limits, payload shape checks, and contract versioning.

### 2. Autonomous runner currently does not execute accepted plans

- `autonomous_runner.rs` converts accepted outcomes into text summaries only.
- For non-hello-world goals, the loop never dispatches plan actions, so AUT-04/AUT-05 are still open.

### 3. Execution primitives already exist in service layer

- Mutations: `ProgramService::propose_edit`
- Verify: `ProgramService::verify`
- Compile: `ProgramService::compile`
- Simulate: `ProgramService::simulate`
- History/checkpoints: `list_history`, `list_checkpoints`, `undo`, `redo`, `restore_checkpoint`, `diff_versions`
- Query context for inspect-like actions: `program_overview`, `search_nodes`, `semantic_query`

### 4. Session/transcript model is text-first and lacks structured stop reasons

- `ProjectAgentSession` stores `run_status`, `active_goal`, and transcript messages.
- Stop details are free-form notes, not machine-readable terminal reason codes.

### 5. Dashboard and agent chat responses already expose planner metadata

- `schema/agent_control.rs` and `schema/dashboard.rs` include `planner` payloads.
- Phase 15 can extend this shape (or add sibling fields) to expose execution outcome/terminal reason details.

## Recommended Executor Design

Introduce an `autonomy_executor` module that accepts `(program_id, agent_id, plan)` and returns typed per-action outcomes.

Dispatch mapping:
- `mutate_batch` -> `ProposeEditRequest` via `to_propose_edit_request()`, enforce committed/valid semantics
- `verify` -> `ProgramService::verify` with normalized scope defaults
- `compile` -> `ProgramService::compile` with validated opt-level/entry args
- `simulate` -> `ProgramService::simulate` with guarded function/input handling
- `inspect` -> safe read-only query path (`program_overview`, optional search/semantic summary)
- `history` -> explicit operation mapping (`list_entries`, `list_checkpoints`, `undo`, `redo`, `restore_checkpoint`, `diff`)

Key rules:
- Unknown/invalid action payloads become structured action failures, not panics.
- Every dispatched action emits structured evidence (`action_index`, `kind`, status, summary/error, timing).
- Mutating actions must continue to rely on existing server validation/atomicity rules (no bypass).

## Stop Reason Taxonomy (Phase 15 baseline)

Recommend stable machine-readable terminal codes:
- `completed`
- `planner_rejected_non_retryable`
- `planner_rejected_retry_budget_exhausted`
- `action_failed_retryable`
- `action_failed_non_retryable`
- `verify_failed`
- `retry_budget_exhausted`
- `operator_stopped`
- `runner_internal_error`

Each code should include a human-readable message plus optional details (`attempt`, `action_index`, `planner_failure_code`).

## Transcript Evidence Contract

For each autonomous attempt, capture:
- attempt number and retry budget state
- planner result (`accepted|failed`, version, action count, failure code)
- per-action execution rows (`index`, `kind`, `status`, concise outcome)
- verify outcome when run
- terminal stop reason code + message

This evidence should be available through existing transcript APIs and preferably projected into structured response fields for operator tooling.

## Risks and Mitigations

- Risk: action dispatch drift from schema contract.
  Mitigation: centralize action-to-service mapping in one executor module with unit tests per action kind.
- Risk: retry loop semantics become non-deterministic.
  Mitigation: explicit max attempt budget + deterministic transition rules between plan/apply/verify/replan.
- Risk: operator ambiguity on why runs stopped.
  Mitigation: enum-based stop reason codes plus transcript evidence snapshots.
- Risk: regression of command-path hello-world behavior.
  Mitigation: keep deterministic command path intact and maintain existing phase10/phase14 integration tests.

## Testing Priorities

1. Unit tests for executor dispatch covering all supported action kinds and failure normalization.
2. Integration test for non-command autonomous run that applies planner mutation actions and reaches terminal success.
3. Integration test for bounded retries (planner rejection or action failure) that ends with explicit stop reason code.
4. Integration test ensuring transcript evidence includes attempt/action/terminal reason details.
5. Regression tests for existing explicit command prompts and dashboard chat planner metadata.

## Recommended Execution Order

1. **Plan 15-01:** Implement generic action executor and execution/stop-reason schema foundation.
2. **Plan 15-02:** Integrate bounded `plan -> apply -> verify -> replan` loop into autonomous runner + API views.
3. **Plan 15-03:** Lock behavior with integration tests and operator documentation for stop reasons/evidence.

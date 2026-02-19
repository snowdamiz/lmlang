# Phase 16: Verify/Compile Repair Loop - Research

**Researched:** 2026-02-19
**Domain:** Autonomous retry-loop diagnostics, verification/compile feedback, and targeted replanning
**Confidence:** HIGH

## Summary

Phase 15 delivered bounded `plan -> apply -> verify -> replan` execution with typed stop reasons and attempt evidence, but Phase 16 still needs stronger repair intelligence:

- Verify currently runs as a post-execution gate, not explicitly after each mutating batch.
- Planner replans without structured diagnostics from the last failed verify/compile attempt.
- Compile and verify details are captured per action, but the next planner prompt does not consume that data deterministically.
- Retry transitions are deterministic, but “targeted repair” behavior is still mostly implicit and text-driven.

Phase 16 should close these gaps by turning verify/compile failures into machine-readable repair context that is fed back into the next planning step.

## Requirements Mapping

| Requirement | Planning implication |
|-------------|----------------------|
| AUT-07 | Verify diagnostics must run automatically around mutation execution and be fed into the next planner attempt |
| AUT-08 | Compile/run failure diagnostics must be captured in structured form and drive focused repair actions instead of generic retries |

## Existing Architecture Signals

### 1. Executor already captures rich per-action details

- `autonomy_executor.rs` emits typed action rows with optional JSON `detail` payloads.
- Failures include `AutonomyExecutionError` with stable code + retryable flag.

### 2. Runner loop has deterministic transitions but limited repair context

- `autonomous_runner.rs` handles planner rejection, action failure, verify gate failure, and retry exhaustion.
- Replanning uses short transcript notes (for example “verify gate failed; replanning”), which are insufficient for targeted fixes.

### 3. Planner prompt currently receives goal + transcript only

- `autonomy_planner.rs::plan_for_prompt` builds prompt context from goal and recent transcript strings.
- No structured “latest diagnostics” block is passed in.

### 4. Session model can hold structured outcomes

- `project_agent.rs` stores attempts and top-level execution outcome.
- This is a good anchor for generating “repair context” from latest failed attempt rows.

### 5. Integration harness already supports deterministic planner assertions

- Existing phase15 tests use a mock planner server and inspect planner requests.
- This enables contract-level assertions that phase16 replanning includes verify/compile diagnostics.

## Recommended Phase 16 Design

1. Add a normalized repair-context representation derived from the latest attempt:
   - Failing action kind/index
   - Verify diagnostics summary (error count + sample/type buckets)
   - Compile diagnostics summary (entry function, diagnostic count, key messages)
   - Retry position (`attempt`, `max_attempts`)

2. Feed repair context into planner prompts deterministically:
   - Extend planner prompt builder with a structured “Latest execution diagnostics” block.
   - Keep format stable and concise to reduce planner variance.

3. Strengthen automatic verify behavior around mutation actions:
   - Ensure mutation execution is followed by verify evidence capture in the same attempt payload.
   - Preserve fail-fast + retry semantics while guaranteeing diagnostics are persisted.

4. Add targeted repair transitions:
   - Use failure class + diagnostics to shape replanning notes and stop details.
   - Cleanly stop on unsafe/non-retryable conditions with explicit reason codes/details.

5. Preserve phase15 compatibility:
   - Keep command-path and existing successful autonomous runs stable.
   - Keep response metadata optional and backward-compatible.

## Risks and Mitigations

- Risk: larger prompt payloads increase planner nondeterminism.
  Mitigation: pass condensed structured diagnostics instead of raw full logs.
- Risk: verify/compile diagnostics shape changes break tests/docs.
  Mitigation: define stable schema fields and assert them in integration tests.
- Risk: over-retrying on repeated identical diagnostics.
  Mitigation: include repeat-signature detection and explicit retry-budget termination messaging.
- Risk: regressions to phase15 stop-reason behavior.
  Mitigation: retain transition table tests and add phase16-specific scenarios.

## Testing Priorities

1. Unit tests for repair-context extraction from attempt action rows.
2. Integration test proving failed verify diagnostics appear in next planner request.
3. Integration test proving compile failure diagnostics drive targeted replan and eventual terminal state.
4. Integration test ensuring clean exit on non-retryable/unsafe conditions with explicit stop reason.
5. Regression tests for deterministic command-path and phase15 success/failure flows.

## Recommended Execution Order

1. **Plan 16-01:** Diagnostics capture foundation and verify-feedback normalization.
2. **Plan 16-02:** Planner feedback injection and targeted repair-loop transitions.
3. **Plan 16-03:** Integration hardening, contract assertions, and operator docs.

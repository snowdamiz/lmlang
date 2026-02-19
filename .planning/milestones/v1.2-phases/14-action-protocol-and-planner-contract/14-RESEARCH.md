# Phase 14: Action Protocol and Planner Contract - Research

**Researched:** 2026-02-19
**Domain:** Autonomous plan contract, server-side validation, and planner routing
**Confidence:** HIGH

## Summary

Phase 14 should replace free-form/non-deterministic autonomous chat behavior with a strict plan contract that the server validates before executing anything.

Current autonomy is constrained by command heuristics:
- `handlers/agent_control.rs` only maps chat text to fixed hello-world commands (`create`, `compile`, `run`).
- `autonomous_runner.rs` only allows one-line command outputs and a tiny fixed command vocabulary.
- `llm_provider.rs` returns plain assistant text and has no JSON contract enforcement.
- `handlers/dashboard.rs` orchestration path assumes chat text and command keywords, not executable action plans.

The existing graph mutation/verify/compile APIs are already sufficient as execution primitives. Phase 14 should define the planner contract and routing so Phase 15 can execute generic plans deterministically.

## Requirements Mapping

| Requirement | Planning implication |
|-------------|----------------------|
| AUT-01 | Non-command natural-language prompts must route into a planner path that returns structured plan output or structured failure metadata |
| AUT-02 | Planner output must be versioned and validated server-side (schema + semantic guards) before any action execution |
| AUT-03 | Plan schema must support ordered multi-step actions (mutation batches + tool calls) beyond hello-world-only commands |

## Existing Architecture Signals

### 1. Command-path bottleneck
- `maybe_execute_agent_chat_command` performs keyword matching for hello-world behavior and returns plain text summaries.
- Non-command text falls through to external LLM chat and remains non-executable.

### 2. Autonomous loop bottleneck
- `autonomous_runner::decide_next_step` prompts for one-line command decisions.
- Allowed outputs are hardcoded (`create hello world program`, `compile program`, `run program`, `done`, `clarify`).

### 3. Execution primitives already available
- Mutations: `POST /programs/{id}/mutations`
- Verify: `POST /programs/{id}/verify`
- Simulate: `POST /programs/{id}/simulate`
- Compile: `POST /programs/{id}/compile`
- History/query endpoints already exist for observability and diagnostics

This means Phase 14 can focus on contract + routing without inventing new low-level graph APIs.

## Recommended Contract Shape (v1)

Introduce a versioned planner envelope in server schema, for example:
- `version`: explicit contract version string
- `goal`: normalized user goal
- `actions`: ordered list of executable actions
- `stop_reason`: optional terminal reason when no safe plan is possible
- `notes`: optional planner rationale

Action variants should cover Phase 14 needs while staying executor-friendly:
- `mutate_batch` (payload maps to existing `ProposeEditRequest` semantics)
- `verify`
- `compile`
- `simulate`
- `history_query` or `inspect` (read-only context calls)

## Validation Requirements

Server-side validation should include both schema and semantic checks:
- version is recognized
- action list is non-empty unless returning structured failure
- max action count guardrail (to prevent runaway plans)
- each action payload is internally valid and bounded
- mutation actions are shape-compatible with existing mutation contract
- explicit failure response includes reason code + user-safe message

## Routing Strategy

Phase 14 routing should preserve backward compatibility while enabling AUT-01:
- Keep deterministic hello-world command path for explicit command text (short-term safety).
- Route non-command prompts to planner contract path.
- If planner output validates, surface structured plan metadata to transcript/response for executor consumption.
- If planner output is invalid or unsafe, return structured failure (not free-form fallback).

## Risks and Mitigations

- Risk: Planner emits malformed JSON.
  Mitigation: strict parse + validation errors with structured reason codes.
- Risk: Planner emits syntactically valid but unsafe/oversized plan.
  Mitigation: semantic constraints (action count/size limits, allowed action whitelist).
- Risk: Regression of existing hello-world flows.
  Mitigation: maintain command fast-path + integration tests for legacy behavior.
- Risk: Ambiguous UI behavior in dashboard chat.
  Mitigation: include planner action summary + explicit fail reason in response payload.

## Testing Priorities

1. Unit tests for planner schema parsing and validation failures.
2. Integration test: non-command prompt routes to planner path and yields structured plan/failure response.
3. Integration test: planner rejects invalid version/invalid action payload.
4. Regression test: `create hello world program`, `compile program`, and `run program` continue to work.
5. Contract test: multi-step plan fixture (>=2 actions) validates successfully.

## Recommended Execution Order

1. **Plan 14-01:** Define versioned schema + validator primitives + documentation.
2. **Plan 14-02:** Integrate planner generation/routing into agent and dashboard chat paths.
3. **Plan 14-03:** Add structured failure/observability fields and end-to-end contract tests including calculator-oriented multi-step plan fixture.

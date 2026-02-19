# Roadmap: lmlang (v1.2 Autonomous Program Synthesis)

## Overview

This milestone closes the core capability gap: agents currently route through fixed hello-world commands or plain text chat. v1.2 introduces a structured planner/executor architecture so the system can autonomously attempt open-ended program creation tasks from natural-language requests.

## Phases

- [x] **Phase 14: Action Protocol and Planner Contract** - Define and implement a strict, versioned planning schema and routing contract for autonomous build intents (completed 2026-02-19)
- [x] **Phase 15: Generic Graph Build Executor** - Implement deterministic execution of planner-produced mutation/tool action sequences with bounded retries and stop reasons (completed 2026-02-19)
- [x] **Phase 16: Verify/Compile Repair Loop** - Integrate automatic verification/compile feedback and targeted repair iteration logic (completed 2026-02-19)
- [x] **Phase 17: Acceptance Benchmarks and Attempt Visibility** - Validate autonomy on calculator and other benchmark prompts with clear timeline observability (completed 2026-02-19)

## Phase Details

### Phase 14: Action Protocol and Planner Contract
**Goal**: Convert natural-language requests into schema-valid, executable action plans.
**Depends on**: Existing v1.0/v1.1 backend baseline
**Requirements**: AUT-01, AUT-02, AUT-03
**Success Criteria**:
1. Planner response schema is versioned, documented, and validated server-side before execution
2. Non-command prompts route through planner path and produce executable plans or explicit structured failure
3. Plan format supports multi-step edits and tool calls beyond hello-world hardcoded commands

### Phase 15: Generic Graph Build Executor
**Goal**: Execute planner actions as safe deterministic server operations against program graphs.
**Depends on**: Phase 14
**Requirements**: AUT-04, AUT-05, AUT-06
**Success Criteria**:
1. Executor applies generic mutation batches from plan actions through existing edit APIs
2. Bounded autonomy loop (`plan -> apply -> verify -> replan`) runs with configurable retry budget
3. Terminal states include explicit reason codes and actionable transcript artifacts

### Phase 16: Verify/Compile Repair Loop
**Goal**: Make autonomous attempts resilient by integrating verification and compiler feedback.
**Depends on**: Phase 15
**Requirements**: AUT-07, AUT-08
**Success Criteria**:
1. Verify runs automatically after each batch and diagnostics flow back into the next planning step
2. Compile/run failure diagnostics are captured and used for targeted repair attempts
3. Loop exits cleanly on success, exhausted retries, or unsafe conditions

### Phase 17: Acceptance Benchmarks and Attempt Visibility
**Goal**: Prove user-facing autonomous capability with benchmark tasks and transparent attempt history.
**Depends on**: Phase 16
**Requirements**: AUT-09, AUT-10, AUT-11
**Success Criteria**:
1. `Create a simple calculator` produces a real autonomous build attempt with calculator-specific structure and verify/compile attempt
2. Two additional benchmark prompts run through the same generic pipeline with persisted attempt records
3. Timeline/history views expose each autonomous step, outputs, and final outcome for operator review

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 14. Action Protocol and Planner Contract | 3/3 | Complete | 2026-02-19 |
| 15. Generic Graph Build Executor | 3/3 | Complete    | 2026-02-19 |
| 16. Verify/Compile Repair Loop | 3/3 | Complete | 2026-02-19 |
| 17. Acceptance Benchmarks and Attempt Visibility | 3/3 | Complete    | 2026-02-19 |

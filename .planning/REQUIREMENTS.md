# Requirements: lmlang

**Defined:** 2026-02-19
**Milestone:** v1.2 Autonomous Program Synthesis
**Core Value:** AI agents can build, modify, verify, and execute programs from natural-language goals with full graph-level awareness

## v1.2 Requirements

### Natural-Language Goal to Plan

- [ ] **AUT-01**: User can submit a natural-language build request and receive a structured autonomous execution attempt instead of a chat-only response.
- [x] **AUT-02**: Planner output must conform to a versioned JSON schema that is validated server-side before any execution.
- [x] **AUT-03**: Planner can decompose a request into multi-step graph/tool actions spanning more than one function or edit batch.

### Generic Autonomous Execution

- [x] **AUT-04**: Executor can apply generic mutation batches from planner output (not limited to hello-world command heuristics).
- [x] **AUT-05**: Executor runs a bounded loop (`plan -> apply -> verify -> replan`) until success or retry budget exhaustion.
- [x] **AUT-06**: Execution failures produce explicit stop reason codes and transcript evidence for operator inspection.

### Verification, Compile, and Repair

- [x] **AUT-07**: After each mutation batch, verify runs automatically and feeds diagnostics back into the next planning step.
- [x] **AUT-08**: Compile and optional run results are captured and can trigger targeted repair iterations when failures occur.

### Acceptance and Observability

- [ ] **AUT-09**: Timeline/history records each autonomous step (prompt context, planned actions, applied mutations, verify/compile outputs, terminal status).
- [ ] **AUT-10**: Prompt `Create a simple calculator` triggers a real autonomous build attempt that creates calculator-related program structure and reaches verify/compile attempt.
- [ ] **AUT-11**: At least two additional non-trivial benchmark prompts (for example string utility and state-machine tasks) trigger autonomous build attempts through the same generic path.

## Future Requirements

- Natural-language test-case generation and auto-evaluation harness
- Automatic retrieval of reusable graph patterns across existing programs
- Multi-agent decomposition for parallel autonomous construction

## Out of Scope (v1.2)

- Unbounded fully autonomous operation without operator stop controls
- Arbitrary host shell command execution from model output
- Cross-tenant/cloud scheduler and fleet orchestration

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| AUT-01 | Phase 14 | Complete (14-02) |
| AUT-02 | Phase 14 | Complete (14-01) |
| AUT-03 | Phase 14 | Complete (14-01) |
| AUT-04 | Phase 15 | Complete (15-01, 15-03) |
| AUT-05 | Phase 15 | Complete (15-02, 15-03) |
| AUT-06 | Phase 15 | Complete (15-01, 15-02, 15-03) |
| AUT-07 | Phase 16 | Complete (16-01, 16-02, 16-03) |
| AUT-08 | Phase 16 | Complete (16-01, 16-02, 16-03) |
| AUT-09 | Phase 17 | Planned |
| AUT-10 | Phase 17 | Planned |
| AUT-11 | Phase 17 | Planned |

**Coverage:**
- v1.2 requirements: 11 total
- Mapped to phases: 11
- Unmapped: 0

---
*Requirements defined: 2026-02-19*

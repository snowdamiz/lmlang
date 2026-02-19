# lmlang

## What This Is

An AI-native programming system where programs are persistent dual-layer graphs (semantic + executable) manipulated by AI agents through a structured tool API and compiled to native binaries via LLVM.

## Core Value

AI agents can build, modify, verify, and execute programs from natural-language goals with full graph-level awareness.

## Current Milestone: v1.2 Autonomous Program Synthesis

**Goal:** Enable autonomous end-to-end program construction from open-ended user prompts (for example, "create a simple calculator") instead of the current hello-world command path.

**Target features:**
- Structured planner contract for turning natural-language goals into executable graph-edit plans
- Generic executor loop that can apply arbitrary mutation batches, verify, and retry
- Compile/run feedback integration for self-repair
- Acceptance benchmarks proving non-trivial autonomous program creation attempts

**Milestone pivot rationale (2026-02-19):**
- v1.1 dashboard work shipped Phase 10 successfully, but current agent behavior is constrained to a fixed hello-world command set.
- Product priority is now aligned to the core language promise: autonomous program creation.

## Requirements

### Validated

- v1.0 baseline platform completed and archived in `.planning/archive/milestones/v1.0-milestone/`
- Core tool API exists for agent/program/lock/mutation/verification operations
- v1.1 Phase 10 dashboard shell shipped and documented

### Active

- v1.2 autonomy requirements in `.planning/REQUIREMENTS.md`
- v1.2 phase roadmap in `.planning/ROADMAP.md`
- Phase 14 planning target: planner contract and action protocol

### Deferred

- Remaining v1.1 dashboard phases (11-13) are deferred until autonomous build capability is operational

### Out of Scope

- Fully unsupervised long-running goals with no operator control in v1.2
- Multi-tenant auth/RBAC and cloud-distributed orchestration in v1.2
- Arbitrary host shell/code execution directly from model output

## Constraints

- Language: Rust
- Persisted graph model remains the system of record for program state
- All autonomous edits must pass server-side validation before commit
- Safety-first failure handling with clear stop reasons and transcript evidence
- Compilation target remains LLVM IR -> native binary

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Pivot from v1.1 dashboard continuation to v1.2 autonomy | Current flow cannot fulfill core product promise of autonomous program creation | Accepted |
| Introduce a strict planner output schema | Prevent brittle free-form text parsing and enable deterministic execution routing | Accepted |
| Continue phase numbering from 14 | Preserve roadmap continuity across milestones | Accepted |
| Include calculator benchmark as hard acceptance criteria | Directly validate the user-facing capability gap that triggered this milestone | Accepted |

---
*Last updated: 2026-02-19 after milestone pivot to v1.2*

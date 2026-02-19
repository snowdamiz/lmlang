# lmlang

## What This Is

An AI-native programming system where programs are persistent dual-layer graphs (semantic + executable) manipulated by AI agents through a structured tool API and compiled to native binaries via LLVM.

## Core Value

AI agents can build, modify, verify, and execute programs from natural-language goals with full graph-level awareness.

## Milestone Status

**Latest shipped milestone:** v1.2 Autonomous Program Synthesis (2026-02-19)

**Delivered capability:**
- Structured planner contract for turning natural-language goals into executable graph-edit plans
- Generic executor loop that can apply arbitrary mutation batches, verify, and retry
- Compile/run feedback integration for self-repair
- Acceptance benchmarks proving non-trivial autonomous program creation attempts with operator timeline visibility

**Archive references:**
- `.planning/milestones/v1.2-ROADMAP.md`
- `.planning/milestones/v1.2-REQUIREMENTS.md`
- `.planning/milestones/v1.2-phases/`

**Next focus:** Define the next milestone requirements and roadmap.

## Requirements

### Validated

- v1.0 baseline platform completed and archived in `.planning/archive/milestones/v1.0-milestone/`
- Core tool API exists for agent/program/lock/mutation/verification operations
- v1.1 Phase 10 dashboard shell shipped and documented
- v1.2 autonomous planner/executor/repair milestone shipped and archived in `.planning/milestones/`

### Active

- Next milestone requirements (to be defined)
- Next milestone roadmap (to be defined)

### Deferred

- Remaining v1.1 dashboard phases (11-13) are deferred until autonomous build capability is operational

### Out of Scope

- Fully unsupervised long-running goals with no operator control until explicitly prioritized
- Multi-tenant auth/RBAC and cloud-distributed orchestration until explicitly prioritized
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
| Archive v1.2 planning artifacts as milestone history | Keep active planning context lightweight while preserving full execution evidence | Accepted |

---
*Last updated: 2026-02-19 after v1.2 milestone completion*

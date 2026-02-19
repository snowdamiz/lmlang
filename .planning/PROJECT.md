# lmlang

## What This Is

An AI-native programming system where programs are persistent dual-layer graphs (semantic + executable) manipulated by AI agents through a structured tool API and compiled to native binaries via LLVM.

## Core Value

AI agents can build, modify, and verify programs of arbitrary size with perfect local and global awareness.

## Current Milestone: v1.1 Agent Control Dashboard

**Goal:** Deliver a dashboard where humans can spawn, supervise, and control AI agents that operate on lmlang programs.

**Target features:**
- Agent orchestration dashboard (spawn, configure, monitor, control)
- Human-in-the-loop approval flow for agent-proposed graph edits
- Execution telemetry and diagnostics for runs, locks, conflicts, and verification
- Reusable workflow templates for common AI-driven development tasks

**Phase 10 delivery status (2026-02-19):**
- Unified dashboard route shipped at `/programs/{id}/dashboard`
- Endpoint-first Operate tab shipped on existing API contracts
- Observe tab reuse shipped via existing observability UI route
- Verification evidence published in `.planning/phases/10-unified-dashboard-shell-endpoint-first-operate-tab/10-VERIFICATION.md`

## Requirements

### Validated

- v1.0 baseline platform completed and archived in `.planning/archive/milestones/v1.0-milestone/`
- Core tool API exists for agent/program/lock/mutation/verification operations

### Active

- v1.1 UI requirements in `.planning/REQUIREMENTS.md`
- v1.1 phase roadmap in `.planning/ROADMAP.md`
- Phase 11 planning target: approval gates and change control

### Out of Scope

- Full autonomous operation without human oversight
- Multi-tenant auth/RBAC system in v1.1
- Cloud-distributed agent orchestration in v1.1

## Constraints

- Language: Rust
- Existing server APIs should be reused before adding new endpoints
- UI must expose clear state for agent actions and safety gates
- Compilation target remains LLVM IR -> native binary

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Pivot v1.1 to UI-first milestone | Core backend exists but workflow is not controllable by users | Completed |
| Keep human approval in the loop for agent edits | Safety and trust while operating on persistent program graphs | Accepted |
| Continue phase numbering from 10 | Preserve continuity with completed v1.0 roadmap | Accepted |
| Reuse observability route inside dashboard Observe tab | Avoid backend duplication and preserve proven graph/query UX | Implemented in Phase 10 |

---
*Last updated: 2026-02-19 after Phase 10 execution*

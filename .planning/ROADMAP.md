# Roadmap: lmlang (v1.1 Agent Control Dashboard)

## Overview

This milestone upgrades the existing graph visibility UI into a unified operator dashboard at `/programs/{id}/dashboard`. The current observability UI is reused as the `Observe` module, while new agent/workflow controls are added in `Operate` using existing backend endpoints first.

## Phases

- [x] **Phase 10: Unified Dashboard Shell & Endpoint-First Operate Tab** - Build `/programs/{id}/dashboard` with `Operate` + `Observe`, reusing observability and existing APIs (completed 2026-02-19)
- [ ] **Phase 11: Approval Gates & Change Control** - Add diff review, approval/rejection, and rollback controls inside `Operate`
- [ ] **Phase 12: Run Lifecycle, Timeline, and Diagnostics** - Add only missing lifecycle/timeline APIs and wire full run control + diagnostics
- [ ] **Phase 13: Workflow Templates & UX Hardening** - Add reusable templates and polish the full operator workflow across both tabs

## Phase Details

### Phase 10: Unified Dashboard Shell & Endpoint-First Operate Tab
**Goal**: Users can access one dashboard per program with `Operate` and `Observe` modules
**Depends on**: v1.0 tool API, v1.0 observability UI
**Requirements**: UI-01, UI-02
**Success Criteria**:
1. `/programs/{id}/dashboard` serves a unified shell with `Operate` and `Observe` tabs
2. `Observe` tab reuses existing observability graph/query UX and endpoints
3. `Operate` tab uses existing endpoints for orchestration primitives: `/agents/register`, `/agents`, `/agents/{agent_id}`, `/programs/{id}/locks`, `/programs/{id}/mutations`, `/programs/{id}/verify`, `/programs/{id}/simulate`, `/programs/{id}/compile`, `/programs/{id}/history`

### Phase 11: Approval Gates & Change Control
**Goal**: Agent edits are safely gated by human review inside the unified dashboard
**Depends on**: Phase 10
**Requirements**: UI-04, UI-05, UI-06
**Success Criteria**:
1. Proposed changes render as structured before/after diffs
2. Approval/rejection actions are persisted with actor and reason
3. Undo/rollback controls are exposed in the same `Operate` run context

### Phase 12: Run Lifecycle, Timeline, and Diagnostics
**Goal**: Operators can control run lifecycle and diagnose progress/failures quickly
**Depends on**: Phase 10
**Requirements**: UI-03, UI-07, UI-08, UI-09
**Success Criteria**:
1. Pause/resume/stop controls work end-to-end via dedicated run lifecycle APIs
2. Timeline view shows ordered run events (tool calls, verification actions, errors, approvals)
3. Lock/conflict dashboard highlights blocked runs and impacted resources using existing lock data plus run timeline context

**API Strategy**:
- Reuse existing endpoints first (`/agents`, `/locks`, `/mutations`, `/verify`, `/simulate`, `/compile`, `/history`)
- Add backend APIs only for missing concepts: run lifecycle (pause/resume/stop) and timeline events

### Phase 13: Workflow Templates & UX Hardening
**Goal**: Repeated AI workflows become fast, consistent, and reliable in the unified dashboard
**Depends on**: Phase 10, Phase 11, Phase 12
**Requirements**: UI-10
**Success Criteria**:
1. Users can create/select templates for common operations
2. Template launch pre-fills run settings and reduces setup errors
3. End-to-end UI flow (`Operate` -> `Observe` -> review -> verify) is covered by integration tests

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 10. Unified Dashboard Shell & Endpoint-First Operate Tab | 4/4 | Complete | 2026-02-19 |
| 11. Approval Gates & Change Control | 0/0 | Planned | — |
| 12. Run Lifecycle, Timeline, and Diagnostics | 0/0 | Planned | — |
| 13. Workflow Templates & UX Hardening | 0/0 | Planned | — |

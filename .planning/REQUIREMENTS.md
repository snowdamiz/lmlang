# Requirements: lmlang

**Defined:** 2026-02-19
**Milestone:** v1.1 Agent Control Dashboard
**Core Value:** AI agents can build, modify, and verify programs of arbitrary size with perfect local and global awareness

## v1.1 Requirements

### Unified Dashboard (Operate + Observe)

- [x] **UI-01**: User can view all registered agents and active sessions in the `Operate` tab with status (`idle`, `running`, `blocked`, `error`)
- [x] **UI-02**: User can spawn a new agent run from `Operate` by selecting target program, workflow template, and task prompt
- [ ] **UI-03**: User can pause, resume, and stop an active agent run from the dashboard once run lifecycle APIs are available

### Human-in-the-Loop Change Control

- [ ] **UI-04**: User can review proposed graph mutations as a structured diff before apply
- [ ] **UI-05**: User can approve or reject a proposed change with a recorded reason
- [ ] **UI-06**: User can undo or rollback from the dashboard using existing checkpoint/history mechanisms

### Run Monitoring and Diagnostics

- [ ] **UI-07**: User can inspect a run timeline that includes lifecycle transitions, tool calls, verification steps, and errors
- [ ] **UI-08**: User can view lock/conflict state for agent sessions and identify blocked operations
- [ ] **UI-09**: User can trigger verify/simulate/compile actions and see results linked to the run

### Workflow Templates

- [ ] **UI-10**: User can create and reuse workflow templates for common tasks (plan phase, execute phase, verify work)

## Future Requirements

- Multi-user collaboration and permissions
- Remote/cloud worker pools for agents
- Fully autonomous background execution policies

## Out of Scope (v1.1)

- Enterprise auth/RBAC
- Billing/quotas
- Distributed control plane

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| UI-01 | Phase 10 | Complete |
| UI-02 | Phase 10 | Complete |
| UI-03 | Phase 12 | Planned |
| UI-04 | Phase 11 | Planned |
| UI-05 | Phase 11 | Planned |
| UI-06 | Phase 11 | Planned |
| UI-07 | Phase 12 | Planned |
| UI-08 | Phase 12 | Planned |
| UI-09 | Phase 12 | Planned |
| UI-10 | Phase 13 | Planned |

**Coverage:**
- v1.1 requirements: 10 total
- Mapped to phases: 10
- Unmapped: 0

---
*Requirements defined: 2026-02-19*

---
phase: 10-unified-dashboard-shell-endpoint-first-operate-tab
status: passed
verified_on: 2026-02-19
requirements: [UI-01, UI-02]
automated:
  - cargo test --package lmlang-server --test integration_test
manual_checks:
  - Open /programs/{id}/dashboard and exercise Operate + Observe tab workflow
---

# Phase 10 Verification

## Scope

This verification confirms Phase 10 requirements for the unified dashboard shell and endpoint-first Operate behavior:
- UI-01: agent/session visibility with actionable statuses
- UI-02: run setup and launch controls using existing endpoints

The verification also confirms Observe reuse and route coexistence with no backend API expansion.

## Requirement Evidence Matrix

| Requirement | Evidence Type | Evidence | Result |
|-------------|---------------|----------|--------|
| UI-01 | Automated | `phase10_dashboard_routes_serve_shell_and_assets` validates Operate mounts and tab shell | Pass |
| UI-01 | Automated | `phase10_dashboard_operate_static_contract_has_endpoint_first_hooks` validates status labels (`idle`,`running`,`blocked`,`error`) and endpoint hooks | Pass |
| UI-01 | Automated | `phase10_dashboard_and_observe_routes_coexist_with_reuse_contract` validates Observe reuse contract and shared route availability | Pass |
| UI-01 | Manual | Dashboard shows registered agents, selected agent badge, and status progression during actions | Pass |
| UI-02 | Automated | Static contract test validates run setup affordances (`Workflow Template`, `Task Prompt`) and endpoint-first actions | Pass |
| UI-02 | Manual | Operate run setup saved/previewed, then action panels execute existing APIs with output snapshots | Pass |

## Automated Test Results

Command:

```bash
cargo test --package lmlang-server --test integration_test
```

Observed outcome:
- 24 tests passed, 0 failed
- Includes Phase 10 route, static contract, and observe-reuse coexistence checks
- No regressions in existing observability, mutation, verify, simulate, compile, or history integration tests

## Manual Smoke Checklist

1. Open `http://localhost:3000/programs/{id}/dashboard`
2. Confirm Operate and Observe tabs render
3. In Operate:
   - Register an agent
   - Select workflow template and task prompt
   - Run at least one endpoint action (locks, mutation dry-run, verify, simulate, compile, or history)
   - Confirm status badge and timeline update
4. Switch to Observe and confirm observability UI renders for the same program id
5. Switch back to Operate and confirm selected agent + run setup context are preserved

## Deferred Items (Planned Follow-on Phases)

- Phase 11:
  - approval/rejection workflow for proposed mutations
  - structured before/after diff review and rollback controls in Operate
- Phase 12:
  - run lifecycle APIs (pause/resume/stop)
  - full timeline event stream and richer diagnostics panels

## Conclusion

Phase 10 requirements UI-01 and UI-02 are met. The dashboard now provides a unified Operate/Observe shell, endpoint-first action orchestration against existing APIs, and explicit verification evidence for follow-on phase planning.

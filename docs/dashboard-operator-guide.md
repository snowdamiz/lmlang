# Dashboard Operator Guide

This guide describes how to use the unified dashboard at `/programs/{id}/dashboard`.

## Purpose

The dashboard combines:
- `Operate`: endpoint-first controls for agent and graph orchestration actions.
- `Observe`: the existing observability UI reused in-context for graph/query inspection.

Phase 10 intentionally uses existing API contracts only. Run lifecycle APIs (pause/resume/stop) are deferred to later phases.

## Prerequisites

1. Start the server:

```bash
cargo run -p lmlang-server
```

2. Create/load a program (replace IDs as needed):

```bash
curl -sX POST localhost:3000/programs \
  -H 'content-type: application/json' \
  -d '{"name":"dashboard-demo"}'

curl -sX POST localhost:3000/programs/1/load \
  -H 'content-type: application/json' \
  -d '{}'
```

3. Open:

`http://localhost:3000/programs/1/dashboard`

## Dashboard Layout

## Header and context strip

The header shows:
- Program id badge
- Selected agent badge
- Current status badge (`idle`, `running`, `blocked`, `error`)
- Workflow template badge

The context strip summarizes:
- active program id
- endpoint-first mode
- Observe reuse path

## Operate tab

Operate contains five panels:

1. Agents & Session State
2. Run Setup
3. Actions
4. Timeline / History Preview
5. Request / Response Output

## Observe tab

Observe embeds `/programs/{id}/observability` and keeps the same program context.

Use either:
- tab switch to Observe, or
- `Open current program in Observe` link from Operate output.

## Core workflow

## 1) Register/select an agent

In **Agents & Session State**:
- enter optional name
- click `Register Agent`
- select the agent card

`Deregister Selected` removes the agent and releases held locks.

## 2) Configure run setup

In **Run Setup**:
- choose workflow template (`Execute Phase`, `Plan Phase`, `Verify Work`)
- enter task prompt
- click `Save Run Setup`
- optional: click `Preview Run Context`

This context is attached to output snapshots for action traceability.

## 3) Execute endpoint-first actions

In **Actions** use existing APIs:

### Locks
- Provide function IDs and lock mode
- `Acquire` calls `/programs/{id}/locks/acquire`
- `Release` calls `/programs/{id}/locks/release`
- `List` calls `/programs/{id}/locks`

Lock actions require a selected agent and send `X-Agent-Id`.

### Mutations
- Paste mutation JSON array
- `Dry Run` calls `/programs/{id}/mutations` with `dry_run: true`
- `Commit` calls `/programs/{id}/mutations` with `dry_run: false`

If an agent is selected, mutation requests include `X-Agent-Id`.

### Verify / Simulate / Compile / History
- `Verify` -> `/programs/{id}/verify`
- `Simulate` -> `/programs/{id}/simulate`
- `Compile` -> `/programs/{id}/compile`
- `History` -> `/programs/{id}/history`

## 4) Inspect timeline and output

Each action records:
- timestamped timeline entry
- endpoint target
- status outcome
- detail text

Output panel captures:
- request payload (including run setup snapshot)
- response payload (success/error)

## 5) Switch to Observe

Use tab switch or output link to inspect graph state after operations.

Tab switches preserve:
- selected agent
- run setup context
- dashboard session state

## Status model

Status values are derived from outcomes:
- `idle`: no active operation or last operation succeeded
- `running`: action in progress
- `blocked`: lock/conflict/missing-agent related failure
- `error`: non-blocking failure state for other errors

## Troubleshooting

## "program is not the active program"

Cause:
- current program in URL does not match active loaded program.

Fix:
- call `POST /programs/{id}/load` for the intended program.

## Lock actions blocked

Cause:
- no selected agent, or lock conflict.

Fix:
- select/register agent first
- inspect lock status via `List`
- release conflicting locks if needed

## Mutation rejected with lock/conflict details

Cause:
- missing write lock, hash conflict, or validation failure.

Fix:
- acquire required write locks
- retry mutation with updated context
- use dry-run first to inspect diagnostics

## Observe not loading in tab

Cause:
- observability route unavailable or server issue.

Fix:
- verify `/programs/{id}/observability` loads directly
- use open-in-new-tab fallback link

## Recommended operator sequence

1. Register/select agent
2. Save run setup
3. Acquire needed locks
4. Run mutation dry-run
5. Commit mutation
6. Verify and/or simulate
7. Compile when ready
8. Review history and Observe graph state

## Deferred capabilities

Phase 10 intentionally defers:
- explicit run lifecycle controls (pause/resume/stop)
- rich event timeline API
- approval/rejection diff gates

These are targeted in Phase 11 and Phase 12.

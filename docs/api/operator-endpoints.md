# Operator Endpoint Reference

This document maps the Phase 10 `Operate` dashboard actions to existing HTTP endpoints.

## Scope

Phase 10 is endpoint-first and reuses existing APIs.
No new backend endpoints were introduced for:
- run lifecycle controls
- timeline event streaming

Those are deferred to later phases.

## Base assumptions

- Server default base URL: `http://localhost:3000`
- Dashboard route: `/programs/{id}/dashboard`
- Most program-scoped endpoints require `{id}` to match active program loaded via `/programs/{id}/load`

## Header behavior

Some operations use selected agent identity:

- Header: `X-Agent-Id: <uuid>`
- Required for:
  - `POST /programs/{id}/locks/acquire`
  - `POST /programs/{id}/locks/release`
- Optional for:
  - `POST /programs/{id}/mutations`

If header is missing where required, server returns an error and dashboard status becomes `blocked`.

## Endpoint matrix

| Area | Method | Path | Operate action |
|------|--------|------|----------------|
| Agents | POST | `/agents/register` | Register agent |
| Agents | GET | `/agents` | Refresh/list agents |
| Agents | DELETE | `/agents/{agent_id}` | Deregister selected agent |
| Locks | POST | `/programs/{id}/locks/acquire` | Acquire locks |
| Locks | POST | `/programs/{id}/locks/release` | Release locks |
| Locks | GET | `/programs/{id}/locks` | List lock status |
| Mutations | POST | `/programs/{id}/mutations` | Dry-run or commit mutation batch |
| Verify | POST | `/programs/{id}/verify` | Verify local/full scope |
| Simulate | POST | `/programs/{id}/simulate` | Interpret function with inputs |
| Compile | POST | `/programs/{id}/compile` | Compile active program |
| History | GET | `/programs/{id}/history` | Fetch edit history |

## Agents

## Register agent

`POST /agents/register`

Request:

```json
{
  "name": "dashboard-operator"
}
```

Response:

```json
{
  "agent_id": "uuid",
  "name": "dashboard-operator",
  "registered_at": "unix-seconds-string"
}
```

## List agents

`GET /agents`

Response:

```json
{
  "agents": [
    {
      "agent_id": "uuid",
      "name": "dashboard-operator"
    }
  ]
}
```

## Deregister agent

`DELETE /agents/{agent_id}`

Response:

```json
{
  "success": true,
  "released_locks": [1, 2]
}
```

## Locks

## Acquire locks

`POST /programs/{id}/locks/acquire`

Headers:

```text
X-Agent-Id: <uuid>
```

Request:

```json
{
  "function_ids": [1, 2],
  "mode": "write",
  "description": "prepare mutation"
}
```

Response:

```json
{
  "grants": [
    {
      "function_id": 1,
      "mode": "write",
      "expires_at": "timestamp"
    }
  ]
}
```

## Release locks

`POST /programs/{id}/locks/release`

Headers:

```text
X-Agent-Id: <uuid>
```

Request:

```json
{
  "function_ids": [1, 2]
}
```

Response:

```json
{
  "released": [1, 2]
}
```

## Lock status

`GET /programs/{id}/locks`

Response:

```json
{
  "locks": [
    {
      "function_id": 1,
      "state": "write",
      "holders": ["uuid"],
      "holder_description": "prepare mutation",
      "expires_at": "timestamp"
    }
  ]
}
```

## Mutations

## Dry-run or commit

`POST /programs/{id}/mutations`

Headers (optional):

```text
X-Agent-Id: <uuid>
```

Request:

```json
{
  "mutations": [
    {
      "type": "AddFunction",
      "name": "from_dashboard",
      "module": 0,
      "params": [],
      "return_type": 7,
      "visibility": "Public"
    }
  ],
  "dry_run": true
}
```

Response:

```json
{
  "valid": true,
  "created": [],
  "errors": [],
  "warnings": [],
  "committed": false
}
```

Set `dry_run: false` to commit.

## Verify

`POST /programs/{id}/verify`

Request:

```json
{
  "scope": "local",
  "affected_nodes": [1, 2]
}
```

Response:

```json
{
  "valid": true,
  "errors": [],
  "warnings": []
}
```

## Simulate

`POST /programs/{id}/simulate`

Request:

```json
{
  "function_id": 1,
  "inputs": [],
  "trace_enabled": true
}
```

Response (shape):

```json
{
  "success": true,
  "result": null,
  "trace": [],
  "error": null,
  "io_log": []
}
```

## Compile

`POST /programs/{id}/compile`

Request:

```json
{
  "opt_level": "O0",
  "debug_symbols": false
}
```

Response:

```json
{
  "binary_path": "./build/...",
  "target_triple": "...",
  "binary_size": 0,
  "compilation_time_ms": 0
}
```

## History

`GET /programs/{id}/history`

Response:

```json
{
  "entries": [],
  "total": 0
}
```

## Common error conditions

## Active program mismatch

Typical message:
- `program {id} is not the active program (active: {active_id})`

Action:
- call `POST /programs/{id}/load` before program-scoped operations.

## Missing agent header

Typical message:
- missing/invalid `X-Agent-Id`

Action:
- register/select agent in dashboard and retry lock-sensitive action.

## Lock or conflict failure

Typical outcomes:
- lock-required failure for mutation
- conflict details for expected hash mismatch

Action:
- reacquire locks, refresh context, optionally dry-run before commit.

## Links to implementation

- Router wiring: `crates/lmlang-server/src/router.rs`
- Lock handler (header extraction): `crates/lmlang-server/src/handlers/locks.rs`
- Mutation handler (optional agent header): `crates/lmlang-server/src/handlers/mutations.rs`
- Dashboard client endpoint map: `crates/lmlang-server/static/dashboard/app.js`

## Deferred endpoint work (not in Phase 10)

Deferred to Phase 11/12:
- run pause/resume/stop endpoints
- structured timeline event API
- approval/rejection mutation workflow endpoints (if needed)

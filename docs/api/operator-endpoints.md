# Operator Endpoint Reference

This document maps the unified `/dashboard` workflow to HTTP endpoints.

## Scope

This reference focuses on dashboard control-loop endpoints for:
- chat-first orchestration,
- project creation and selection,
- provider/model/API credential configuration,
- project-agent assignment,
- start/stop build runs,
- agent chat,
- observe/query flow.

## Base URL

- `http://localhost:3000`

## Dashboard entrypoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/dashboard` | Top-level dashboard shell |
| POST | `/dashboard/ai/chat` | Chat-first orchestration endpoint |
| GET | `/dashboard/app.js` | Dashboard client script |
| GET | `/dashboard/styles.css` | Dashboard CSS |
| GET | `/programs/{id}/dashboard` | Dashboard shell with initial selected project |

## Project management

Most operators should use:

`POST /dashboard/ai/chat`

Request:

```json
{
  "message": "create project hello-world",
  "selected_program_id": null,
  "selected_agent_id": null,
  "selected_project_agent_id": null
}
```

Response:

```json
{
  "success": true,
  "reply": "...",
  "selected_program_id": 1,
  "selected_agent_id": "uuid",
  "selected_project_agent_id": "uuid",
  "actions": ["..."],
  "transcript": [],
  "planner": {
    "status": "accepted",
    "version": "2026-02-19",
    "actions": [
      { "kind": "inspect", "summary": "query='calculator plan', max_results=4" },
      { "kind": "verify", "summary": "scope=Some(Local)" }
    ]
  }
}
```

`planner` is omitted for explicit command-style prompts and included for non-command planner-routed prompts.

Supported orchestration prompts include:
- `create project <name>`
- `register agent <name> provider openrouter model <model> api key <key>`
- `assign agent`
- `start build <goal>`
- `stop build`
- `create hello world program`
- `compile program`
- `run program`

## Planner contract (`AUT-02` / `AUT-03`)

Natural-language build requests are represented by a versioned planner envelope:

```json
{
  "version": "2026-02-19",
  "goal": "create a simple calculator",
  "metadata": {
    "planner": "phase14-planner",
    "model": "openai/gpt-4o-mini"
  },
  "actions": [
    {
      "type": "mutate_batch",
      "request": {
        "mutations": [{ "type": "AddFunction", "name": "add", "module": 0, "params": [], "return_type": 3, "visibility": "Public" }],
        "dry_run": false
      }
    },
    {
      "type": "verify",
      "request": { "scope": "Full" }
    }
  ]
}
```

Contract notes:
- `version` must match the server-supported planner contract version (`2026-02-19`).
- `actions` is an ordered sequence (`max: 32`), validated before any autonomous execution routing.
- Supported action variants: `mutate_batch`, `verify`, `compile`, `simulate`, `inspect`, `history`.
- `mutate_batch` uses the same payload semantics as `POST /programs/{id}/mutations` (`Mutation` / `ProposeEditRequest` shape).
- `verify` uses existing verify scope semantics (`Local` or `Full`).

If no safe plan can be generated, planner output can return structured failure instead of actions:

```json
{
  "version": "2026-02-19",
  "goal": "build unsupported runtime target",
  "actions": [],
  "failure": {
    "code": "unsupported_goal",
    "message": "Requested target is unavailable in this environment.",
    "detail": "missing runtime capability: wasm32 host",
    "retryable": false
  }
}
```

Semantic validation errors are machine-readable and include explicit codes, including:
- `unsupported_version`
- `missing_actions`
- `too_many_actions`
- `missing_required_field`
- `invalid_field_value`
- `invalid_action_payload`

## List projects

`GET /programs`

## Create project

`POST /programs`

Request:

```json
{
  "name": "demo"
}
```

## Load selected project

`POST /programs/{id}/load`

## Agent registration and provider config

## Register agent

`POST /agents/register`

Request:

```json
{
  "name": "builder-01",
  "provider": "openrouter",
  "model": "openai/gpt-4o-mini",
  "api_base_url": "https://openrouter.ai/api/v1",
  "api_key": "sk-or-...",
  "system_prompt": "You are a focused build assistant."
}
```

Supported provider values:
- `openrouter`
- `openai_compatible`

## List agents

`GET /agents`

## Get one agent

`GET /agents/{agent_id}`

## Update agent provider config

`POST /agents/{agent_id}/config`

Request:

```json
{
  "provider": "openai_compatible",
  "model": "gpt-4.1-mini",
  "api_base_url": "https://api.openai.com/v1",
  "api_key": "sk-...",
  "system_prompt": "Be concise."
}
```

Notes:
- API keys are persisted in SQLite (`agent_configs`) and survive server restarts.
- API key is never returned in responses.
- For `openrouter`, if `api_base_url` is empty, server defaults to `https://openrouter.ai/api/v1`.

## Project-agent assignment and control

## List assigned agents for project

`GET /programs/{id}/agents`

## Assign agent to project

`POST /programs/{id}/agents/{agent_id}/assign`

## Get assigned agent detail and transcript

`GET /programs/{id}/agents/{agent_id}`

## Start build run

`POST /programs/{id}/agents/{agent_id}/start`

Request:

```json
{
  "goal": "build parser"
}
```

## Stop build run

`POST /programs/{id}/agents/{agent_id}/stop`

Request:

```json
{
  "reason": "manual stop"
}
```

## Chat with assigned agent

`POST /programs/{id}/agents/{agent_id}/chat`

Request:

```json
{
  "message": "create hello world program"
}
```

Command-style prompts:
- `create hello world program`: creates/loads `hello_world`, inserts missing `Return`, verifies full graph.
- `compile program`: compiles with `entry_function = "hello_world"`.
- `run program`: compiles (if needed) and executes produced binary.

Non-command prompts:
- Route through planner contract path (`AUT-01`) and return structured planner metadata in response payloads.
- Planner success includes normalized action summaries (`planner.status = accepted`, `planner.actions[]`).
- Planner failure includes explicit reason code/message (`planner.status = failed`, `planner.failure.code`, `planner.failure.message`).
- No plain external-chat fallback is used for non-command execution intent.

Success response shape (non-command prompt):

```json
{
  "success": true,
  "reply": "Planner accepted 2 action(s) for goal 'build a simple calculator'...",
  "planner": {
    "status": "accepted",
    "version": "2026-02-19",
    "actions": [
      { "kind": "inspect", "summary": "query='calculator requirements', max_results=5" },
      { "kind": "verify", "summary": "scope=Some(Full)" }
    ]
  }
}
```

Failure response shape (invalid planner JSON):

```json
{
  "success": true,
  "reply": "Planner rejected request [planner_invalid_json]: ...",
  "planner": {
    "status": "failed",
    "failure": {
      "code": "planner_invalid_json",
      "message": "Planner response was not valid JSON: ...",
      "retryable": true
    }
  }
}
```

## Autonomous execution metadata (`AUT-06`, `AUT-07`, `AUT-08`)

When autonomous runs complete or stop, agent and dashboard responses include machine-readable execution metadata:

- `session.stop_reason`: terminal code/message for the last run
- `session.execution`: compact summary of the latest attempt (`attempt`, `max_attempts`, action rows)
- `session.execution_attempts`: ordered bounded attempt timeline (each attempt has action rows + stop reason)
- `chat.execution` and `dashboard/ai/chat.execution`: same compact attempt summary on chat surfaces
- `chat.execution_attempts` and `dashboard/ai/chat.execution_attempts`: same bounded timeline history for operator rendering
- `execution.actions[].diagnostics`: compact diagnostics class/retryability/summary per failed action row
- `execution.diagnostics`: latest attempt-level diagnostics summary for quick triage
- `dashboard/ai/chat.diagnostics`: same diagnostics summary exposed at top level for UI convenience

Representative shape:

```json
{
  "session": {
    "run_status": "stopped",
    "stop_reason": {
      "code": "retry_budget_exhausted",
      "message": "retry budget exhausted after action failure: ..."
    },
    "execution": {
      "attempt": 3,
      "max_attempts": 3,
      "planner_status": "accepted",
      "action_count": 2,
      "succeeded_actions": 1,
      "actions": [
        {
          "action_index": 0,
          "kind": "compile",
          "status": "failed",
          "summary": "compile action failed",
          "error_code": "internal_error",
          "diagnostics": {
            "class": "compile_failure",
            "retryable": true,
            "summary": "compile action failed",
            "key_diagnostics": ["internal error: entry function 'missing_entry' not found"]
          }
        },
        {
          "action_index": 1,
          "kind": "verify_gate",
          "status": "failed",
          "summary": "post-execution verify failed with 1 diagnostic(s)",
          "error_code": "validation_failed",
          "diagnostics": {
            "class": "verify_failure",
            "retryable": true,
            "summary": "verify gate reported 1 diagnostic(s)",
            "key_diagnostics": ["[TYPE_MISMATCH] ..."]
          }
        }
      ],
      "diagnostics": {
        "class": "verify_failure",
        "retryable": true,
        "summary": "verify gate reported 1 diagnostic(s)",
        "key_diagnostics": ["[TYPE_MISMATCH] ..."]
      },
      "stop_reason": {
        "code": "retry_budget_exhausted",
        "message": "retry budget exhausted after verify gate failure (attempt 3/3)"
      }
    },
    "execution_attempts": [
      {
        "attempt": 1,
        "max_attempts": 3,
        "planner_status": "accepted",
        "action_count": 2,
        "succeeded_actions": 1,
        "actions": [
          {
            "action_index": 0,
            "kind": "mutate_batch",
            "status": "succeeded",
            "summary": "applied 1 mutation(s): add_function(calculator_add)"
          },
          {
            "action_index": 1,
            "kind": "compile",
            "status": "failed",
            "summary": "compile action failed",
            "error_code": "internal_error"
          }
        ],
        "stop_reason": {
          "code": "action_failed_retryable",
          "message": "compile action failed"
        }
      }
    ]
  }
}
```

### Benchmark timeline examples (`AUT-09`, `AUT-10`, `AUT-11`)

Phase 17 benchmark prompts should produce timeline-visible attempt records through the same planner/executor pipeline:

- Calculator benchmark (`Create a simple calculator`): timeline includes calculator-targeted mutation summary (for example `add_function(calculator_add)`) plus verify/compile rows and terminal outcome.
- String utility benchmark: timeline includes string utility mutation markers (for example `add_function(string_normalize)`) and persisted attempt metadata.
- State-machine/workflow benchmark: timeline includes workflow structure markers (for example `add_function(ticket_state_transition)`) and persisted terminal status.

Representative dashboard chat shape:

```json
{
  "success": true,
  "execution": {
    "attempt": 1,
    "max_attempts": 3,
    "stop_reason": { "code": "completed" }
  },
  "execution_attempts": [
    {
      "attempt": 1,
      "planner_status": "accepted",
      "actions": [
        {
          "kind": "mutate_batch",
          "summary": "applied 1 mutation(s): add_function(string_normalize)"
        },
        {
          "kind": "verify",
          "summary": "verification passed (scope=full)"
        }
      ],
      "stop_reason": { "code": "completed" }
    }
  }
}
```

### Bounded loop model (`AUT-05`)

Autonomous runtime follows:

`plan -> apply actions -> verify gate -> replan (if retryable and budget remains)`

Retry budget defaults to `3` attempts and can be configured with:

- `LMLANG_AUTONOMY_MAX_ATTEMPTS`

### Diagnostics-driven repair flow (`AUT-07` / `AUT-08`)

- Attempt `N=1`: planner prompt contains goal + transcript only (no diagnostics block).
- Retry attempt `N+1`: planner prompt includes a `Latest execution diagnostics` JSON block derived from the latest failed attempt (`action_kind`, `error_class`, `retryable`, `key_diagnostics`, `attempt/max_attempts`).
- Targeted repair retries remain bounded by `max_attempts`; exhausted retries terminate with `retry_budget_exhausted`.
- Non-retryable planner rejections (for example `unsafe_plan`) terminate with `planner_rejected_non_retryable` and include machine-readable detail (`planner_code`, `attempt`, `max_attempts`, validation errors if present).

### Stop reason taxonomy

Terminal `stop_reason.code` values:

- `completed`
- `planner_rejected_non_retryable`
- `planner_rejected_retry_budget_exhausted`
- `action_failed_retryable`
- `action_failed_non_retryable`
- `verify_failed`
- `retry_budget_exhausted`
- `operator_stopped`
- `runner_internal_error`

### Troubleshooting quick map

- `planner_rejected_non_retryable`: prompt/goal unsupported by planner contract; revise goal scope
  - inspect `stop_reason.detail.planner_code` (for example `unsafe_plan`, `unsupported_goal`) before retrying
- `planner_rejected_retry_budget_exhausted`: planner stayed retryable but never produced executable actions
- `action_failed_non_retryable`: action payload invalid or structurally impossible without plan change
- `retry_budget_exhausted`: repeated retryable planner/action/verify failures consumed budget
  - inspect `execution.actions[].diagnostics` and `execution.diagnostics` to identify repeating failure class
- `runner_internal_error`: server-side execution/setup issue (inspect logs + details payload)

Autonomous run behavior:
- Starting a build run (`POST /programs/{id}/agents/{agent_id}/start`) spawns a background loop.
- The loop can execute known build commands (`create hello world program`, `compile program`, `run program`) without waiting for a chat turn.
- For non-hello-world goals, the loop executes validated planner actions via the generic executor and records typed per-attempt evidence.

## Observe integration

The dashboard links selected projects to existing observability endpoints:
- `GET /programs/{id}/observability`
- `GET /programs/{id}/observability/graph`
- `POST /programs/{id}/observability/query`

## Error patterns

Common error responses include:
- project not found,
- agent not found,
- agent not assigned to project,
- incomplete provider config for external chat,
- empty goal/message.

Response envelope:

```json
{
  "success": false,
  "error": {
    "code": "BAD_REQUEST",
    "message": "..."
  }
}
```

## Implementation pointers

- Router: `crates/lmlang-server/src/router.rs`
- Agent handlers: `crates/lmlang-server/src/handlers/agents.rs`
- Project-agent chat handler: `crates/lmlang-server/src/handlers/agent_control.rs`
- Runtime manager: `crates/lmlang-server/src/project_agent.rs`
- Dashboard client: `crates/lmlang-server/static/dashboard/app.js`

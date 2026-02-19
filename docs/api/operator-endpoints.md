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
  "transcript": []
}
```

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
        "mutations": [{ "type": "add_function", "name": "add", "module": 0, "params": [], "return_type": "I32", "visibility": "Public" }],
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
- If provider config is complete (`provider` + `model` + `api_key`), server calls configured OpenAI-compatible `/chat/completions` endpoint.
- If provider config is absent, server returns local fallback guidance text.

Autonomous run behavior:
- Starting a build run (`POST /programs/{id}/agents/{agent_id}/start`) spawns a background loop.
- The loop can execute known build commands (`create hello world program`, `compile program`, `run program`) without waiting for a chat turn.
- If the provider response asks for clarification, the loop records the question, applies a default assumption, and continues autonomously.

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

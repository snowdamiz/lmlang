# Dashboard Operator Guide

This guide describes the unified dashboard entrypoint at `/dashboard`.

## What changed

You no longer need a pre-existing project URL to use the dashboard.

Start at:

`http://localhost:3000/dashboard`

From there, you can:
1. Use one chat box for project + agent + build orchestration.
2. Configure AI providers (OpenRouter/OpenAI-compatible) via chat prompts.
3. Create, compile, and run programs via chat prompts.

## Prerequisites

1. Start the server:

```bash
cargo start-server
```

2. Open the dashboard:

`http://localhost:3000/dashboard`

3. On first open, complete the setup wizard:
- choose provider (`OpenRouter` or `OpenAI-compatible`)
- enter model
- enter API key

## Dashboard areas

## Projects panel

Use this panel to:
- create a new project by name,
- refresh project list,
- select the active project,
- open selected project in Observe.

When you select a project, the dashboard automatically loads it with:
- `POST /programs/{id}/load`

## Agents panel

Use this panel to:
- register global agents,
- configure provider/model/base URL/API key for each registered agent,
- refresh global agent list,
- assign an agent to the selected project,
- inspect assigned agents and their run status.

Assignment is project-scoped and uses:
- `POST /programs/{id}/agents/{agent_id}/assign`

Provider config uses:
- `POST /agents/{agent_id}/config`

## Build Control + Chat panel

Primary control loop:
- Send orchestration prompts (project, agent, assignment, start/stop) through chat.
- Send build prompts through chat:
  - `build calculator workflow`
  - `add parser entrypoint and verify`
  - `compile and run main entry function`
- Build prompts are planner-routed; configured provider/model/API key is required.

These actions use:
- `POST /programs/{id}/agents/{agent_id}/start`
- `POST /programs/{id}/agents/{agent_id}/stop`
- `POST /programs/{id}/agents/{agent_id}/chat`

## End-to-end workflow

1. In chat, send: `create project calculator-demo`.
2. In chat, send: `register agent builder provider openrouter model openai/gpt-4o-mini api key <your-key>`.
3. In chat, send: `assign agent`.
4. In chat, send: `start build calculator workflow`.
5. In chat, send: `build calculator workflow with verify and compile`.
6. In chat, send: `compile active entry function`.
7. In chat, send: `run active entry function`.
8. Open Observe and run query: `calculator`.
9. In chat, send: `stop build` when done.

## Status badges

Header badges show:
- active project,
- active selected assigned agent,
- current run state.

Common run states:
- `idle`: assigned, not actively building.
- `running`: build run started.
- `stopped`: run explicitly stopped.

## Troubleshooting

## Cannot assign agent

Likely causes:
- no project selected,
- no registered agents,
- invalid/removed agent id.

Actions:
- select a project,
- register/refresh agents,
- retry assignment.

## Cannot start build

Likely causes:
- no assigned agent selected,
- empty goal,
- selected project does not exist anymore.

Actions:
- select assigned agent,
- provide goal,
- refresh projects and reselect.

## Chat fails

Likely causes:
- no project/agent selected,
- empty message,
- agent not assigned to selected project.

Actions:
- select project and assigned agent,
- ensure non-empty chat message,
- reassign agent if needed.

## Observe link unavailable

Likely cause:
- no project selected.

Action:
- select a project first; link updates automatically.

## Planner build flow fails

Likely causes:
- project not selected,
- project not loaded,
- planner output rejected by contract or temporary verification/query failure.

Actions:
- reselect project,
- resend a clear build prompt with goal and constraints,
- refresh and open Observe to confirm graph state.

## API note

- Build runs now include an autonomous background loop after `start build`.
- The loop progresses planner actions without requiring an additional chat turn.
- If provider output asks a blocking clarification question, the loop logs it and applies a default assumption so execution continues.
- Agent API keys are persisted in SQLite and survive server restarts.

## Related docs

- API map: `docs/api/operator-endpoints.md`
- Server routing: `crates/lmlang-server/src/router.rs`
- Dashboard client: `crates/lmlang-server/static/dashboard/app.js`

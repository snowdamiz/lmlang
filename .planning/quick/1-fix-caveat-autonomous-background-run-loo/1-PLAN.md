---
title: Quick task 1 - autonomous loop + API key persistence
created_at: 2026-02-19
must_haves:
  truths:
    - Agent provider config survives server restarts using SQLite persistence.
    - Running project agents can progress work without requiring a new chat turn.
    - Clarification prompts are handled autonomously by applying assumptions.
  artifacts:
    - crates/lmlang-server/src/agent_config_store.rs
    - crates/lmlang-server/src/autonomous_runner.rs
    - crates/lmlang-server/src/handlers/agent_control.rs
    - crates/lmlang-server/src/handlers/agents.rs
    - crates/lmlang-server/src/state.rs
    - crates/lmlang-storage/src/migrations/003_agent_config_store.sql
  key_links:
    - crates/lmlang-storage/src/schema.rs -> crates/lmlang-storage/src/migrations/003_agent_config_store.sql
    - crates/lmlang-server/src/state.rs -> crates/lmlang-server/src/agent_config_store.rs
    - crates/lmlang-server/src/state.rs -> crates/lmlang-server/src/autonomous_runner.rs
---

## Task 1
files: crates/lmlang-storage/src/schema.rs, crates/lmlang-storage/src/migrations/003_agent_config_store.sql, crates/lmlang-server/src/agent_config_store.rs, crates/lmlang-server/src/concurrency/agent.rs, crates/lmlang-server/src/state.rs
action: Add SQLite-backed agent config persistence and hydrate in-memory registry at startup.
verify: Register/update agent config, restart AppState on same DB path, check /agents still shows api_key_configured=true.
done: completed

## Task 2
files: crates/lmlang-server/src/autonomous_runner.rs, crates/lmlang-server/src/project_agent.rs, crates/lmlang-server/src/handlers/agent_control.rs, crates/lmlang-server/src/handlers/dashboard.rs
action: Add background run loop to execute goal steps autonomously and self-resolve clarification prompts with assumptions.
verify: Start a hello-world scaffold run and observe transcript advances without manual chat.
done: completed

## Task 3
files: crates/lmlang-server/tests/integration_test.rs, docs/api/operator-endpoints.md, docs/dashboard-operator-guide.md, README.md
action: Add regression tests and update docs to reflect autonomous behavior and persisted API key storage.
verify: cargo test -q -p lmlang-server --test integration_test
done: completed

# lmlang

**lmlang is a first AI-native programming language/runtime**: programs are persistent graphs that agents can inspect, modify, verify, and execute directly.

## Purpose

Traditional languages treat code as text and AI as an external assistant.
lmlang treats the program as structured data first, so AI agents can operate with precise graph-level awareness.

Core goals:
- agent-native program editing and execution,
- persistent, queryable program state,
- safe multi-agent workflows with verification and history.

## Language overview

A lmlang program has two synchronized graph layers:
- **Compute graph** for executable logic and data/control flow.
- **Semantic graph** for higher-level meaning (functions, types, docs, contracts, relationships).

This makes the system suitable for autonomous agents that need machine-usable program structure, not just source text.

## Quickstart

### 1. Start server

```bash
cargo start-server
```

Open dashboard:

```bash
open http://localhost:3000/dashboard
```

### 2. Run hello-world flow in dashboard chat

1. `create project hello-world`
2. `register agent builder provider openrouter model openai/gpt-4o-mini api key <your-key>`
3. `assign agent`
4. `start build hello world bootstrap`

The autonomous run loop can scaffold/compile/run based on goal intent.

## Project layout

- `/crates/lmlang-core`: language graph model and type system
- `/crates/lmlang-storage`: SQLite + in-memory persistence
- `/crates/lmlang-check`: type checking, interpretation, contracts
- `/crates/lmlang-codegen`: LLVM/native compilation
- `/crates/lmlang-server`: HTTP API + dashboard/observability
- `/crates/lmlang-cli`: CLI workflows

## Docs

- Language + architecture deep dive: `/docs/language-architecture.md`
- Dashboard operator guide: `/docs/dashboard-operator-guide.md`
- API endpoints: `/docs/api/operator-endpoints.md`

## Build and test

```bash
cargo test -q -p lmlang-core
cargo test -q -p lmlang-storage
cargo test -q -p lmlang-check
cargo test -q -p lmlang-codegen --test integration_tests
cargo test -q -p lmlang-server --test integration_test
cargo test -q -p lmlang-server --test concurrency
```

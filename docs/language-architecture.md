# lmlang Language Architecture

This page captures the technical language/runtime details that are intentionally kept out of the top-level README.

## Program model

lmlang programs are graph structures, not parsed source files.

`ProgramGraph` contains two synchronized layers:
- **Compute graph**: executable nodes (`ComputeNode`) connected by flow edges (`FlowEdge`).
- **Semantic graph**: high-level entities (`SemanticNode`) connected by semantic relations (`SemanticEdge`).

The two layers are kept consistent through explicit propagation events/flush.

## Type system

`TypeId` is nominal identity. Built-ins:
- `0: Bool`
- `1: I8`
- `2: I16`
- `3: I32`
- `4: I64`
- `5: F32`
- `6: F64`
- `7: Unit`
- `8: Never`

`LmType` supports scalar, array, struct, enum, pointer, function signatures, unit, and never.

## Compute operations

Core (`ComputeOp`) includes:
- constants/arithmetic/comparison/logic/shifts,
- control flow (`IfElse`, `Loop`, `Match`, `Branch`, `Jump`, `Phi`),
- memory (`Alloc`, `Load`, `Store`, `GetElementPtr`),
- calls (`Call`, `IndirectCall`, `Return`, `Parameter`),
- console/file I/O,
- closures (`MakeClosure`, `CaptureAccess`),
- contracts (`Precondition`, `Postcondition`, `Invariant`).

Structured (`StructuredOp`) includes struct/array create-get-set, casts, and enum helpers.

## Edge model

`FlowEdge` kinds:
- `Data { source_port, target_port, value_type }`
- `Control { branch_index }`

## Functions and modules

- Modules are hierarchical with visibility (`ModuleTree`).
- Functions define params/return/module membership.
- Closures support captures + parent function references.

## Storage and persistence

`lmlang-storage` provides:
- `GraphStore` trait,
- `SqliteStore` (persistent),
- `InMemoryStore` (tests/ephemeral).

SQLite schema includes:
- language graph tables (`programs`, `types`, `modules`, `functions`, `compute_nodes`, `flow_edges`, `semantic_nodes`, `semantic_edges`),
- history/checkpoint tables (`edit_log`, `checkpoints`),
- agent provider config table (`agent_configs`).

Migrations:
- `/crates/lmlang-storage/src/migrations/001_initial_schema.sql`
- `/crates/lmlang-storage/src/migrations/002_edit_history.sql`
- `/crates/lmlang-storage/src/migrations/003_agent_config_store.sql`

## Server/API surface

Entrypoint:
- `cargo start-server`

Environment:
- `LMLANG_DB_PATH` (default: `lmlang.db`)
- `LMLANG_PORT` (default: `3000`)

High-level route groups:
- Agents + config (`/agents/...`)
- Dashboard (`/dashboard`)
- Program CRUD/load (`/programs/...`)
- Project-agent lifecycle/chat (`/programs/{id}/agents/...`)
- Mutations, locks, queries, verify, simulate, compile, contracts, history
- Observability UI/query (`/programs/{id}/observability...`)

See:
- `/docs/api/operator-endpoints.md`

## Runtime behavior notes

- Build runs can be autonomous after `start build` via a background loop.
- If clarification is needed during autonomous execution, default assumptions are applied so the loop can proceed.
- Agent API keys are persisted in SQLite and are not returned by API responses.

## Workspace crates

- `/crates/lmlang-core`
- `/crates/lmlang-storage`
- `/crates/lmlang-check`
- `/crates/lmlang-codegen`
- `/crates/lmlang-server`
- `/crates/lmlang-cli`

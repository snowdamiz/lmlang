# lmlang

An AI-native programming system where programs are persistent graphs instead of text files.

This repository implements:
- a dual-layer program model (`semantic` + `compute` graphs),
- type checking and interpretation,
- LLVM native compilation,
- SQLite persistence with edit history/checkpoints,
- multi-agent concurrency primitives,
- an HTTP/JSON tool API and observability UI,
- a CLI compiler.

## What the "language" is

lmlang programs are built as graph structures, not parsed from source text.

The core abstraction is `ProgramGraph`:
- **Compute graph**: executable nodes (`ComputeNode`) and flow edges (`FlowEdge`) for data/control flow.
- **Semantic graph**: higher-level entities (`SemanticNode`) and semantic relations (`SemanticEdge`) for modules/functions/types/specs/tests/docs.

Both layers live in `crates/lmlang-core` and are synchronized through explicit propagation events/flush.

## Workspace layout

Rust workspace crates:

- `crates/lmlang-core`: graph model, op vocabulary, type system, module/function model.
- `crates/lmlang-storage`: `GraphStore` trait + SQLite/in-memory backends, migrations, hashing/dirty tracking.
- `crates/lmlang-check`: static type checker, interpreter, contracts, property testing.
- `crates/lmlang-codegen`: graph -> LLVM IR -> native binary (inkwell + system linker), incremental compilation engine.
- `crates/lmlang-server`: axum HTTP server exposing tool-style endpoints and observability UI.
- `crates/lmlang-cli`: `lmlang` binary for direct compile-from-DB workflows.

## Program model details

### Types

`TypeId` is nominal identity. Built-ins are pre-registered:
- `0: Bool`
- `1: I8`
- `2: I16`
- `3: I32`
- `4: I64`
- `5: F32`
- `6: F64`
- `7: Unit`
- `8: Never`

`LmType` supports:
- scalar, array, struct, enum,
- pointer,
- function signature,
- unit/never.

### Compute operations

Tier 1 core ops (`ComputeOp`) include:
- constants, arithmetic, comparison, logic, shifts,
- control flow (`IfElse`, `Loop`, `Match`, `Branch`, `Jump`, `Phi`),
- memory (`Alloc`, `Load`, `Store`, `GetElementPtr`),
- calls (`Call`, `IndirectCall`, `Return`, `Parameter`),
- console/file I/O,
- closures (`MakeClosure`, `CaptureAccess`),
- contracts (`Precondition`, `Postcondition`, `Invariant`).

Tier 2 structured ops (`StructuredOp`) include:
- struct create/get/set,
- array create/get/set,
- cast,
- enum create/discriminant/payload.

### Edges

`FlowEdge` has two kinds:
- `Data { source_port, target_port, value_type }`
- `Control { branch_index }`

### Functions/modules

- Modules are hierarchical (`ModuleTree`) with visibility.
- Functions have params/return type/module membership.
- Closures support captures and parent function references.

## Storage and persistence

`lmlang-storage` provides:
- `GraphStore` trait (swappable backend boundary),
- `SqliteStore` (persistent),
- `InMemoryStore` (tests/ephemeral use).

SQLite schema includes:
- `programs`, `types`, `modules`, `functions`,
- `compute_nodes`, `flow_edges`,
- `semantic_nodes`, `semantic_edges`,
- `edit_log`, `checkpoints`.

Migrations are in:
- `crates/lmlang-storage/src/migrations/001_initial_schema.sql`
- `crates/lmlang-storage/src/migrations/002_edit_history.sql`

## Tooling surfaces

### 1) HTTP server (`lmlang-server`)

Entrypoint:
- `cargo run -p lmlang-server`

Environment:
- `LMLANG_DB_PATH` (default `lmlang.db`)
- `LMLANG_PORT` (default `3000`)

Route groups:
- Agents: `/agents/register`, `/agents/{agent_id}`, `/agents`
- Programs: `/programs`, `/programs/{id}`, `/programs/{id}/load`
- Mutations: `/programs/{id}/mutations`
- Locks: `/programs/{id}/locks/acquire|release|locks`
- Queries: `/programs/{id}/overview|nodes/{node_id}|functions/{func_id}|neighborhood|search|semantic`
- Verify: `/programs/{id}/verify`, `/programs/{id}/verify/flush`
- Simulate: `/programs/{id}/simulate`
- Compile: `/programs/{id}/compile`, `/programs/{id}/dirty`
- Contracts: `/programs/{id}/property-test`
- History: `/programs/{id}/history|undo|redo|checkpoints|diff`
- Observability UI/data:
  - `/programs/{id}/observability`
  - `/programs/{id}/observability/graph`
  - `/programs/{id}/observability/query`
  - static assets under `/programs/{id}/observability/...`

### Mutation request shape

`POST /programs/{id}/mutations` accepts:
- `mutations: [Mutation...]`
- `dry_run: bool`
- optional `expected_hashes` (for optimistic conflict checks with agent sessions)

`Mutation` variants:
- `InsertNode`, `RemoveNode`, `ModifyNode`
- `AddEdge`, `AddControlEdge`, `RemoveEdge`
- `AddFunction`, `AddModule`

### 2) CLI (`lmlang`)

Run help:
- `cargo run -p lmlang-cli -- --help`

Compile command:
- `cargo run -p lmlang-cli -- compile --db ./lmlang.db --program 1 --opt-level O2`

This loads a persisted graph from SQLite and compiles it via `lmlang-codegen`.

### 3) Rust library APIs

Key entry points:
- graph construction/mutation in `lmlang-core::ProgramGraph`
- full-graph type check: `lmlang_check::typecheck::validate_graph`
- interpretation: `lmlang_check::interpreter::Interpreter`
- compilation:
  - `lmlang_codegen::compile`
  - `lmlang_codegen::compile_to_ir`
  - `lmlang_codegen::compile_incremental`

### 4) Observability UI

Serve server, then open:
- `http://localhost:3000/programs/{id}/observability`

It renders a dual-layer graph and supports natural-language query requests against the observability endpoint.

## Quickstart

### Prerequisites

- Rust toolchain (edition 2021 workspace)
- LLVM 21 for inkwell (`inkwell` is compiled with feature `llvm21-1`)
- C toolchain (`cc`) for final linking

Repo-local config sets:
- `.cargo/config.toml` -> `LLVM_SYS_211_PREFIX="/opt/homebrew/opt/llvm"`

Adjust this for your machine if needed.

### Build and test

```bash
cargo test -q -p lmlang-core
cargo test -q -p lmlang-storage
cargo test -q -p lmlang-check
cargo test -q -p lmlang-codegen --test integration_tests
cargo test -q -p lmlang-server --test integration_test
cargo test -q -p lmlang-server --test concurrency
```

### Minimal end-to-end HTTP workflow

```bash
# 1) Start server
cargo run -p lmlang-server
```

In another shell:

```bash
# 2) Create program
curl -sX POST localhost:3000/programs \
  -H 'content-type: application/json' \
  -d '{"name":"demo"}'

# 3) Load as active program (replace ID)
curl -sX POST localhost:3000/programs/1/load \
  -H 'content-type: application/json' \
  -d '{}'

# 4) Add function
curl -sX POST localhost:3000/programs/1/mutations \
  -H 'content-type: application/json' \
  -d '{"mutations":[{"type":"AddFunction","name":"main","module":0,"params":[],"return_type":7,"visibility":"Public"}],"dry_run":false}'

# 5) Verify
curl -sX POST localhost:3000/programs/1/verify \
  -H 'content-type: application/json' \
  -d '{"scope":"full"}'

# 6) Compile
curl -sX POST localhost:3000/programs/1/compile \
  -H 'content-type: application/json' \
  -d '{"opt_level":"O0","debug_symbols":false}'
```

## Concurrency model

- Agents register and receive UUIDs.
- Locks are function-scoped read/write with TTL + expiry sweep.
- Lock-aware mutations use `X-Agent-Id`.
- Optional optimistic conflict detection compares expected per-function hashes before commit.

## Contracts and testing

- Contract nodes are first-class ops in the compute graph.
- Interpreter enforces preconditions/postconditions/invariants during simulation.
- Property testing endpoint executes seed + randomized inputs and returns failures with counterexamples and optional traces.

## Notable current behavior/caveats

- `POST /programs/{id}/compile` currently uses full compile (`compile`), not server-wired incremental compile (`compile_incremental` exists in service/codegen APIs).
- In `ProgramService::program_overview`, the program name is currently hardcoded as `"default"` (TODO note in code).
- Interpreter file I/O ops (`FileOpen`, `FileRead`, `FileWrite`, `FileClose`) are placeholder behaviors (return `I64(0)`).
- Codegen file I/O paths are currently stub-style helpers around libc calls.

## Where to read next

- Core model: `crates/lmlang-core/src/graph.rs`, `crates/lmlang-core/src/ops.rs`
- Type checker/interpreter/contracts: `crates/lmlang-check/src/`
- Storage/migrations: `crates/lmlang-storage/src/`
- Server routes/schemas: `crates/lmlang-server/src/router.rs`, `crates/lmlang-server/src/schema/`
- LLVM pipeline: `crates/lmlang-codegen/src/compiler.rs`, `crates/lmlang-codegen/src/codegen.rs`

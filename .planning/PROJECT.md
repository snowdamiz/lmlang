# lmlang

## What This Is

An AI-native programming system where programs are persistent dual-layer graphs — not text files — manipulated by AI agents through a structured tool API. The system stores programs as a semantic knowledge graph (intent, contracts, relationships) paired with an executable computational graph (typed operations, data/control flow), compiled to native binaries via LLVM. Humans observe and query the program through graph visualization and natural language, but never write "code."

## Core Value

AI agents can build, modify, and verify programs of arbitrary size with perfect local and global awareness — no context window limits, no hallucinated syntax, no lost state.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] Dual-layer graph representation (semantic + executable) with bidirectional propagation
- [ ] Embedded graph storage with typed nodes, edges, embeddings, and summaries
- [ ] AI tool API: retrieve_subgraph, simulate, propose_structured_edit, verify_and_propagate, optimize
- [ ] Full contract system: type checking, pre/post-conditions, invariants, property-based testing
- [ ] Graph interpreter for development execution
- [ ] LLVM compilation pipeline: graph → LLVM IR → native binary
- [ ] Incremental recompilation via dirty node tracking
- [ ] Multi-agent concurrent graph manipulation with consistency guarantees
- [ ] Graph visualization (DAG viewer) for human observability
- [ ] Natural language query interface over the program graph
- [ ] General-purpose computational primitives (~30+ op nodes: arithmetic, control flow, memory, I/O)

### Out of Scope

- Human-readable source language / text syntax — the graph is the program, not a transpilation target
- Built-in AI agent loop — the system exposes an API, external LLMs drive it
- IDE / text editor integration — humans observe via visualization, not traditional editing
- Cloud/distributed deployment — single-machine prototype first

## Context

The architecture draws from several converging research directions:
- **Knowledge graphs over codebases** (Neo4j + AST extraction) for semantic understanding
- **LLVM/MLIR intermediate representations** for the executable layer
- **Graph-of-Thoughts / GraphReader** for agent reasoning patterns
- **Search-based synthesis** (Compiler.next) for optimization via evolutionary/MCTS mutation
- **Agentic tool-calling** (MCP, function calling) for the AI manipulation interface

The system is built in Rust, using:
- **petgraph** or similar for in-memory graph structures
- **SQLite** for persistent storage (swappable to Neo4j/Memgraph later)
- **inkwell** (LLVM bindings) for native compilation
- **serde** for graph serialization

The dual-layer architecture means:
- **Layer 1 (Semantic Knowledge Graph)**: Modules, functions, data types, specs, tests, docs. Each node has embeddings, compact summaries, and formal JSON-schema interfaces. Edges encode relationships: `calls`, `data_flows_to`, `depends_on`, `implements`, `test_for`.
- **Layer 2 (Executable Computational Graph)**: DAG + cycles (loops) of atomic typed operations. Nodes are ops (add, mul, load/store, branch, loop, call, allocate). Edges are data dependencies + control flow. No text syntax — structured records only.

Bidirectional propagation: editing either layer synchronizes the other. High-level intent changes expand into executable subgraphs; low-level optimizations bubble up summaries and contracts.

## Constraints

- **Language**: Rust — performance-critical, strong LLVM ecosystem, good for compiler tooling
- **Storage**: Embedded/lightweight for prototype — must be swappable to full graph DB without architectural changes
- **AI interface**: Model-agnostic external API — must work with any LLM that supports structured output / tool calling
- **Compilation**: Must target LLVM IR — no custom backends, leverage existing optimization infrastructure

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust implementation | Performance, LLVM bindings (inkwell), type safety for compiler internals | — Pending |
| Embedded storage over graph DB | Simpler prototype, portable, no external dependencies — architecture allows swap later | — Pending |
| External API over built-in agent | Model-agnostic, lets the system focus on being a great program store/compiler, not an AI orchestrator | — Pending |
| Bidirectional layer propagation | AI should work at any abstraction level — intent or implementation — and changes flow automatically | — Pending |
| Full contract system from v1 | Contracts are the mechanism that makes multi-agent concurrent editing safe — can't defer this | — Pending |
| LLVM for compilation | Industry-standard optimizer, multi-target support, proven at scale — no reason to reinvent | — Pending |

---
*Last updated: 2026-02-17 after initialization*

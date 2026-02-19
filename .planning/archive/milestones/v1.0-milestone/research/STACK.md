# Stack Research

**Domain:** AI-native programming language / graph-based program representation system
**Researched:** 2026-02-17
**Confidence:** MEDIUM-HIGH (versions verified via crates.io/docs.rs; some crates lack Context7 verification)

## Recommended Stack

### Language & Runtime

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| Rust | stable (1.84+) | Implementation language | Memory safety without GC is critical for a compiler/runtime. Ownership model naturally fits graph lifecycle management. Ecosystem has mature LLVM bindings, graph libs, and async runtime. |
| Tokio | 1.43+ (LTS) | Async runtime | De facto standard async runtime for Rust. Required by axum, tower, and most async crates. LTS releases guarantee stability. Multi-agent concurrent graph manipulation needs async task scheduling. |

### Graph Data Structures (In-Memory)

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| petgraph | 0.8.3 | Core in-memory graph representation | The dominant Rust graph library (3.5M+ downloads). Provides `StableGraph` which preserves node/edge indices across removals -- critical for a persistent graph where IDs must be stable. Built-in serde support (`serde-1` feature), DOT export, and algorithms (toposort, cycles, traversals). Actively maintained with 0.8.x releases throughout 2025. |

**Confidence:** HIGH -- petgraph is the uncontested standard. StableGraph is specifically designed for the use case where indices must remain valid, which maps directly to lmlang's persistent node IDs.

**Key petgraph features to enable:**
```toml
petgraph = { version = "0.8", features = ["serde-1"] }
```

**Why not alternatives:**
- `graphlib` -- Less mature, smaller ecosystem, no serde support
- `gryf` -- Interesting design (Result-based API) but much smaller community, unproven at scale
- `graph-api-lib` -- Backend-agnostic abstraction layer; adds indirection we don't need since we own the graph layer
- `graphina` -- Network science focused (community detection, link prediction), not general purpose

### Graph Storage (Persistent)

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| rusqlite | 0.38.0 | Primary persistent storage backend | Ergonomic SQLite bindings with `bundled` feature for zero-dependency builds. 44M+ downloads, battle-tested. SQLite's single-file database maps perfectly to "one file per lmlang program." ACID transactions for graph mutation safety. Schema can model adjacency lists efficiently. |
| CozoDB | 0.7.6 | Future graph-native storage (optional) | Embedded graph database in pure Rust with Datalog query language. SQLite backend option means data portability. Graph algorithms (PageRank, shortest path) built in. Consider as optional future backend when graph query complexity outgrows hand-written SQL. |

**Confidence:** HIGH for rusqlite (industry standard). MEDIUM for CozoDB (smaller community, 67K downloads, but technically compelling).

**Why rusqlite over CozoDB as primary:**
1. SQLite is universally understood -- lower barrier for contributors
2. rusqlite has 650x more downloads -- more battle-tested
3. Schema-based storage gives full control over graph representation
4. CozoDB's Datalog query language adds learning curve
5. Migration path: start with rusqlite, add CozoDB as alternative backend later

**Why not other graph databases:**
- `IndraDB` -- Sled/Postgres backends, no SQLite. Sled is not recommended for production by its own author. gRPC adds unnecessary complexity for embedded use.
- `cqlite` -- Pre-release, unstable file format, not production-ready
- `kuzu` -- C++ core with Rust FFI bindings; adds C++ build dependency. Impressive performance but overkill for embedded use.
- Neo4j -- Server-based, not embeddable. Planned as optional future remote backend, not core.

```toml
rusqlite = { version = "0.38", features = ["bundled", "serde_json"] }
```

### LLVM Compilation

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| inkwell | 0.7.1 (crates.io) or 0.8.0 (git master) | Safe LLVM IR generation | The only safe Rust wrapper over LLVM. Strongly typed API catches LLVM errors at Rust compile time. Supports LLVM 11-21. Active development (0.7.x releases in 2025). Used by multiple language projects. |
| llvm-sys | (transitive via inkwell) | Raw LLVM C FFI | Pulled in automatically by inkwell. Version tracks LLVM releases (e.g., 191 = LLVM 19.1.x). |

**Confidence:** HIGH -- inkwell is the established choice for Rust+LLVM projects. No real alternative exists at the same abstraction level.

**LLVM version strategy:** Target LLVM 18 initially.
- LLVM 18 has the widest package manager availability (apt, homebrew, most distros)
- LLVM 19+ has LTO build issues with semi-official binaries on Linux
- LLVM 21 support available when needed

```toml
# Recommended: use crates.io release with LLVM 18
inkwell = { version = "0.7", features = ["llvm18-0"] }

# Alternative: git master for LLVM 18+ features
# inkwell = { git = "https://github.com/TheDan64/inkwell", branch = "master", features = ["llvm18-0"] }
```

**Build requirements:**
- LLVM 18 must be installed on the build machine
- Set `LLVM_SYS_180_PREFIX` environment variable to LLVM install path
- macOS: `brew install llvm@18`
- Ubuntu/Debian: Use https://apt.llvm.org/ packages
- Memory: 16GB+ RAM if building LLVM from source (not needed with package manager)

**Why not Cranelift:**
Cranelift is designed for fast compilation of Rust itself, not as a general-purpose compiler backend for new languages. It lacks the optimization passes, target coverage, and IR flexibility of LLVM. lmlang needs full optimization support for native binary output. However, Cranelift could serve as a future fast-dev-mode JIT backend (compilation ~10x faster than LLVM, code ~14% slower).

### Web API Layer

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| axum | 0.8.x | HTTP API framework for AI agent tool interface | Part of the Tokio project. Built on Tower + Hyper. Clean API design, excellent middleware ecosystem via tower-http. Better DX than Actix-web with nearly identical performance. Ideal for the structured JSON API that AI agents will call. |
| tower-http | 0.6.8 | HTTP middleware (CORS, tracing, compression) | Standard middleware stack for axum. Provides trace logging, CORS, compression, request/response limits out of the box. |
| serde_json | 1.0.x | JSON serialization for API | Industry standard. Required for tool calling API request/response handling. |

**Confidence:** HIGH -- axum is the community-recommended default for new Rust web projects. Tower middleware ecosystem is mature.

```toml
axum = "0.8"
tower-http = { version = "0.6", features = ["cors", "trace", "compression-gzip"] }
```

**Why not alternatives:**
- `Actix-web` -- 10-15% faster raw throughput but worse DX, not in Tokio ecosystem. Performance difference irrelevant for an API serving AI agents (not high-frequency trading).
- `Rocket` -- More opinionated, convention-over-configuration approach. Less composable than axum+tower.
- `Warp` -- Filter-based API is clever but harder to understand/maintain. Less active development.

### Serialization & Data Interchange

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| serde | 1.0.228 | Serialization framework | The Rust serialization standard. 823M+ downloads. Required by virtually every other crate. |
| serde_json | 1.0.149 | JSON serialization | For API payloads, graph export/import, config files. |
| bincode | 2.0.x | Binary serialization | For efficient graph snapshot storage. Faster and more compact than JSON for persistent storage of large graphs. |
| toml | 0.8.x | TOML parsing | For project configuration files (.lmlang.toml or similar). |

**Confidence:** HIGH -- serde ecosystem is the universal choice in Rust.

```toml
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
bincode = "2.0"
toml = "0.8"
```

**Note on graph serialization:** petgraph's `serde-1` feature handles graph serialization correctly -- preserving node/edge indices and using a compact representation (node list + edge list with endpoints). Zero-allocation serialization. Cyclic graphs serialize fine because petgraph uses index-based references, not pointer-based.

### Concurrency

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| dashmap | 6.1.0 | Concurrent hashmap for shared graph metadata | Direct replacement for `RwLock<HashMap>`. 173M+ downloads. Sharded design gives good concurrent read/write performance for multi-agent access to graph node metadata. |
| crossbeam | 0.8.4 | Concurrent utilities (channels, epoch GC) | Standard concurrent programming toolkit. MPMC channels for agent-to-engine communication. Work-stealing deques if we add parallel graph traversal. |
| tokio::sync | (via tokio) | Async-aware locks, semaphores, channels | Built into Tokio. `RwLock` for graph-level locking, `mpsc`/`broadcast` channels for event propagation between agents. |

**Confidence:** HIGH -- all three are industry standards for concurrent Rust.

```toml
dashmap = "6.1"
crossbeam = "0.8"
# tokio::sync included via tokio dependency
```

**Concurrency strategy for multi-agent graph manipulation:**
- Graph structure mutations: `tokio::sync::RwLock` on the graph (write lock for structural changes)
- Node metadata reads: `DashMap` for concurrent read access to node properties
- Agent coordination: `crossbeam-channel` for synchronous inter-agent messages, `tokio::sync::broadcast` for async event propagation
- Transaction isolation: Per-agent transaction contexts with optimistic concurrency control

### Error Handling

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| thiserror | 2.0.18 | Structured error types for library code | Define precise error enums for each subsystem (graph errors, LLVM errors, storage errors, API errors). Callers can match on variants for recovery. v2.0 is the current major release. |
| miette | 7.x | Diagnostic error reporting | Provides rich, user-facing error reports with source spans, labels, and suggestions. Essential for a programming language -- users need to see where in the graph/code the error occurred. |

**Confidence:** HIGH for thiserror (609M+ downloads). MEDIUM for miette (need to verify latest version fits, but it's the standard for compiler diagnostics in Rust).

```toml
thiserror = "2.0"
miette = { version = "7", features = ["fancy"] }
```

**Why thiserror over anyhow for core logic:**
A compiler/language system needs structured errors that downstream code can match on and recover from. `anyhow` erases error types -- fine for CLI main() but wrong for a compiler pipeline where parse errors, type errors, and LLVM errors need distinct handling. Use `anyhow` only in the top-level CLI binary crate, not in library crates.

**Why not snafu:**
snafu combines thiserror + anyhow concepts but adds its own macro complexity. thiserror is simpler, more widely adopted, and v2.0 addresses the main ergonomic gaps.

### Observability

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| tracing | 0.1.41 | Structured logging & instrumentation | De facto standard for Rust observability. Span-based tracing maps naturally to compiler passes and graph operations. Async-aware. 387M+ downloads. |
| tracing-subscriber | 0.3.x | Log output formatting | Companion crate for tracing. Configurable output (pretty, JSON, compact). |

**Confidence:** HIGH -- universal standard.

```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
```

### Graph Visualization

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| petgraph (DOT export) | (included) | DOT format generation | petgraph includes DOT export natively via `petgraph::dot::Dot`. Zero additional dependencies for basic graph visualization. |
| graphviz-rust | 0.9.6 | Rich DOT generation with attributes | When petgraph's built-in DOT export isn't enough (custom styling, subgraphs, colors for dual-layer visualization). Most popular Rust Graphviz crate (27K downloads/mo). |

**Confidence:** MEDIUM -- petgraph's built-in DOT is sufficient for v1. graphviz-rust adds richness later.

```toml
# petgraph DOT export is included by default, no extra dependency
# Add later when rich visualization needed:
# graphviz-rust = "0.9"
```

**Why not layout (pure Rust renderer):**
`layout` renders DOT files without Graphviz installed, but its output quality is lower than Graphviz. For a programming language's visualization, quality matters. Require Graphviz as an optional external dependency.

### Testing

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| proptest | 1.10.0 | Property-based testing | Strategy-based approach is more flexible than quickcheck. Compose generators for complex graph structures. Auto-shrinking finds minimal failing cases. 93M+ downloads. Essential for testing graph invariants and compiler correctness. |
| insta | 1.x | Snapshot testing | For testing compiler output (LLVM IR, graph serialization). Review and approve expected outputs. |
| criterion | 0.5.x | Benchmarking | For graph operation and compilation performance regression testing. |

**Confidence:** HIGH for proptest. MEDIUM for insta/criterion (versions from training data, verify on use).

```toml
[dev-dependencies]
proptest = "1.10"
insta = { version = "1", features = ["json", "yaml"] }
criterion = { version = "0.5", features = ["html_reports"] }
```

### CLI

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| clap | 4.5.x | Command-line argument parsing | 669M+ downloads. Derive-based API for ergonomic CLI definition. Supports subcommands (compile, run, serve, visualize). Auto-generated help. |

**Confidence:** HIGH -- universal standard.

```toml
clap = { version = "4.5", features = ["derive"] }
```

### Identity & Unique IDs

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| uuid | 1.18.1 | Unique node/edge identifiers | v7 UUIDs are time-sortable -- useful for ordering graph mutations chronologically. 374M+ downloads. Serde integration included. |
| ulid | (alternative) | -- | Consider ULID if you need shorter string representation than UUID. Same time-sortability as UUIDv7. |

**Confidence:** HIGH for uuid.

```toml
uuid = { version = "1.18", features = ["v7", "serde"] }
```

### AI Agent Protocol (Future Phase)

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| rmcp | (latest) | Official MCP SDK for Rust | If exposing lmlang as an MCP tool server, this is the official SDK from the Model Context Protocol project. Supports tool definitions, prompts, resources. Eliminates boilerplate for AI tool calling. |
| jsonrpsee | (latest) | JSON-RPC 2.0 server | If building a custom protocol instead of MCP. Async, built on Tokio. Parity-maintained. |

**Confidence:** LOW -- MCP ecosystem is rapidly evolving. Research specific versions at implementation time.

**Strategy:** Start with a plain axum REST API for the tool interface. Add MCP/JSON-RPC as a protocol layer later. Don't couple the core to any specific AI protocol early.

## Supporting Libraries

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| bytes | 1.x | Efficient byte buffers | When handling binary data in the API layer or graph serialization |
| once_cell | 1.x | Lazy statics | Global registries (op node type registry, built-in function table) |
| regex | 1.x | Pattern matching | For natural language query parsing, if implementing basic pattern matching |
| smallvec | 1.x | Stack-allocated small vectors | Optimization for graph node edge lists (most nodes have <8 edges) |
| indexmap | 2.x | Insertion-ordered hash map | Preserve declaration order in graph node properties |

## Alternatives Considered

| Category | Recommended | Alternative | Why Not Alternative |
|----------|-------------|-------------|---------------------|
| Graph library | petgraph | graphlib | Smaller ecosystem, no serde, less algorithm coverage |
| Graph library | petgraph | graph-api-lib | Adds abstraction layer we don't need; own our graph representation |
| Storage | rusqlite | CozoDB | Smaller community, Datalog learning curve; consider as future optional backend |
| Storage | rusqlite | sled | Author explicitly warns it's not production-ready |
| LLVM bindings | inkwell | raw llvm-sys | All-unsafe API, no ergonomic benefit, easy to cause UB |
| LLVM bindings | inkwell | cranelift | Not a general-purpose compiler backend; lacks optimization passes |
| Web framework | axum | actix-web | Not in Tokio ecosystem, steeper learning curve, marginal perf gain irrelevant |
| Error handling | thiserror | anyhow | Type erasure inappropriate for compiler internals; use anyhow only at CLI boundary |
| Error handling | thiserror | snafu | More complex macro system, less widely adopted |
| Concurrency | dashmap | flurry | DashMap has simpler API, more battle-tested (173M vs ~2M downloads) |
| Testing | proptest | quickcheck | Strategy-based composition is more flexible for graph generation |
| Visualization | petgraph DOT | layout (pure Rust) | Lower rendering quality than Graphviz |

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `sled` (as primary storage) | Author warns not production-ready; known data loss bugs | `rusqlite` with bundled SQLite |
| `anyhow` in library crates | Erases error types; compiler needs structured errors for diagnostics | `thiserror` for library crates, `anyhow` only in binary crate `main()` |
| `jsonrpc` (Parity old crate) | Deprecated, unmaintained | `jsonrpsee` if you need JSON-RPC |
| `lspower` | Deprecated, re-merged into `tower-lsp` | `tower-lsp` directly |
| `log` crate directly | Less capable than tracing; no span context | `tracing` (has `log` compatibility layer) |
| Neo4j as primary storage | Server-based, not embeddable, adds deployment complexity | `rusqlite` for embedded; Neo4j only as optional remote backend |
| Rolling your own graph library | petgraph covers all needs; reinventing is months of work | `petgraph::StableGraph` |
| LLVM master/trunk | Unstable, API changes, build issues | Pin to LLVM 18 (latest stable with wide availability) |
| `Rc`/`Arc`-based graph structures | Cyclic references cause memory leaks or require weak refs; serde can't handle cycles | Index-based graphs via petgraph |

## Stack Patterns by Variant

**If building the interpreter-first prototype (recommended):**
- Skip inkwell/LLVM initially
- Use petgraph + rusqlite + axum + serde
- Focus on graph representation and API correctness
- Add LLVM compilation in a later phase

**If prioritizing native compilation early:**
- Add inkwell from the start
- Ensure LLVM 18 is in CI environment
- Gate LLVM features behind a cargo feature flag so interpreter-only builds don't require LLVM

**If targeting MCP protocol for AI agents:**
- Use `rmcp` crate for MCP server implementation
- Expose graph operations as MCP tools
- Still use axum underneath for HTTP transport

**If graph queries become complex (100K+ nodes):**
- Add CozoDB as alternative storage backend
- Use Datalog for recursive graph queries (transitive closure, reachability)
- Keep rusqlite as default for small-to-medium graphs

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| inkwell 0.7.x | LLVM 11-21 via feature flags | Must match system LLVM version exactly |
| axum 0.8.x | tokio 1.x, tower 0.4.x, tower-http 0.6.x | All Tokio ecosystem; versions are coordinated |
| petgraph 0.8.x | serde 1.0.x (via `serde-1` feature) | Serialization format preserves indices |
| rusqlite 0.38.x | SQLite 3.x (bundled) | `bundled` feature includes SQLite, no system dependency |
| dashmap 6.1.x | Works with tokio::sync, crossbeam | Use stable 6.x, not 7.0 release candidates |
| thiserror 2.0.x | miette 7.x for diagnostic display | thiserror defines errors, miette renders them beautifully |

## Installation

```toml
# Cargo.toml

[dependencies]
# Graph
petgraph = { version = "0.8", features = ["serde-1"] }

# Storage
rusqlite = { version = "0.38", features = ["bundled", "serde_json"] }

# LLVM (feature-gated)
inkwell = { version = "0.7", features = ["llvm18-0"], optional = true }

# Web API
axum = "0.8"
tower-http = { version = "0.6", features = ["cors", "trace"] }
tokio = { version = "1", features = ["full"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
bincode = "2.0"
toml = "0.8"

# Concurrency
dashmap = "6.1"
crossbeam = "0.8"

# Error handling
thiserror = "2.0"
miette = { version = "7", features = ["fancy"] }

# Observability
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# CLI
clap = { version = "4.5", features = ["derive"] }

# Identity
uuid = { version = "1.18", features = ["v7", "serde"] }

[dev-dependencies]
proptest = "1.10"
insta = { version = "1", features = ["json"] }
criterion = { version = "0.5", features = ["html_reports"] }

[features]
default = []
llvm = ["dep:inkwell"]
```

```bash
# System dependencies

# macOS
brew install llvm@18
export LLVM_SYS_180_PREFIX=$(brew --prefix llvm@18)

# Ubuntu/Debian
wget https://apt.llvm.org/llvm.sh
chmod +x llvm.sh
sudo ./llvm.sh 18
export LLVM_SYS_180_PREFIX=/usr/lib/llvm-18

# Optional: Graphviz for visualization
brew install graphviz  # macOS
sudo apt install graphviz  # Debian/Ubuntu
```

## Workspace Structure Recommendation

```
lmlang/
  Cargo.toml          # workspace root
  crates/
    lmlang-core/      # Graph types, node definitions, core traits
    lmlang-storage/   # rusqlite backend, storage trait, (future: cozo backend)
    lmlang-compiler/  # inkwell LLVM codegen (feature-gated)
    lmlang-interp/    # Graph interpreter for dev mode
    lmlang-api/       # axum API server, tool definitions
    lmlang-cli/       # clap CLI binary (uses anyhow here)
    lmlang-contracts/ # Type system, pre/post-conditions, invariants
```

This workspace structure allows:
- Independent compilation of subsystems
- Feature-gating LLVM (only `lmlang-compiler` depends on inkwell)
- Clear dependency direction: core <- storage/compiler/interp <- api <- cli
- `thiserror` in all library crates, `anyhow` only in `lmlang-cli`

## Sources

- [TheDan64/inkwell GitHub](https://github.com/TheDan64/inkwell) -- inkwell versions, LLVM support matrix, build requirements (HIGH confidence)
- [inkwell on crates.io](https://crates.io/crates/inkwell) -- version 0.7.1 verified (HIGH confidence)
- [petgraph on crates.io/docs.rs](https://docs.rs/crate/petgraph/latest) -- version 0.8.2-0.8.3 verified (HIGH confidence)
- [petgraph serde serialization source](https://github.com/petgraph/petgraph/blob/master/src/graph_impl/serialization.rs) -- serialization format details (HIGH confidence)
- [rusqlite on crates.io](https://crates.io/crates/rusqlite) -- version 0.38.0 verified (HIGH confidence)
- [CozoDB GitHub](https://github.com/cozodb/cozo) -- version 0.7.6, storage backends, performance (MEDIUM confidence)
- [axum 0.8 announcement](https://tokio.rs/blog/2025-01-01-announcing-axum-0-8-0) -- version and features (HIGH confidence)
- [Tokio releases](https://github.com/tokio-rs/tokio/releases) -- LTS versions 1.43/1.47 (HIGH confidence)
- [dashmap on crates.io](https://crates.io/crates/dashmap) -- version 6.1.0 stable (HIGH confidence)
- [thiserror on crates.io](https://crates.io/crates/thiserror) -- version 2.0.18 (HIGH confidence)
- [tracing on GitHub](https://github.com/tokio-rs/tracing) -- version 0.1.41 (HIGH confidence)
- [proptest on crates.io](https://crates.io/crates/proptest) -- version 1.10.0 (HIGH confidence)
- [clap on crates.io](https://crates.io/crates/clap) -- version 4.5.x (HIGH confidence)
- [uuid on crates.io](https://crates.io/crates/uuid) -- version 1.18.1 (HIGH confidence)
- [tower-http on crates.io](https://crates.io/crates/tower-http) -- version 0.6.8 (HIGH confidence)
- [Cranelift website](https://cranelift.dev/) -- comparison with LLVM (MEDIUM confidence)
- [IndraDB docs](https://indradb.github.io/) -- graph database comparison (MEDIUM confidence)
- [MCP Rust SDK GitHub](https://github.com/modelcontextprotocol/rust-sdk) -- rmcp crate (LOW confidence -- rapidly evolving)
- [serde on crates.io](https://crates.io/crates/serde) -- version 1.0.228 (HIGH confidence)
- [crossbeam on GitHub](https://github.com/crossbeam-rs/crossbeam) -- version 0.8.4 (HIGH confidence)

---
*Stack research for: lmlang -- AI-native programming language with graph-based program representation*
*Researched: 2026-02-17*

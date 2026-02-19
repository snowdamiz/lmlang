# Phase 5: LLVM Compilation Pipeline - Context

**Gathered:** 2026-02-18
**Status:** Ready for planning

<domain>
## Phase Boundary

Compile computational graphs to native binaries through LLVM/inkwell. Each op node maps to LLVM IR, function-scoped Context isolation prevents LLVM types from leaking, and compiled output must match interpreter results for the same inputs. Incremental compilation belongs to Phase 6.

</domain>

<decisions>
## Implementation Decisions

### Binary output format
- Standalone executable — single binary that runs directly
- Static linking — self-contained, no runtime dependencies
- Output placed in deterministic project directory (e.g., `./build/`)
- Debug symbols stripped by default, included only when explicitly requested via flag

### Targets & optimization
- Cross-compilation supported — agent provides LLVM target triple as parameter
- Default target: host machine's native triple
- Optimization via preset levels only (O0/O1/O2/O3) — no custom LLVM pass configuration
- Claude's Discretion: debug vs release mode distinction or single-mode with opt level flag

### Runtime I/O & errors
- Runtime errors (division by zero, null pointer, OOB) abort with descriptive error message and non-zero exit code
- Error messages include the graph node ID that caused the error — agent can map back to graph structure for targeted fixes
- Meaningful exit codes: different codes for success (0), runtime error, trap, etc. — useful for CI and agent inspection
- Claude's Discretion: I/O strategy (direct stdio vs thin runtime library)

### Compilation trigger
- Both HTTP endpoint (POST /programs/{id}/compile) and CLI command (lmlang compile)
- Same underlying pipeline, two entry points — HTTP for agents, CLI for humans
- Compile response includes: binary path, target triple, binary size, compilation time
- Compilation always runs type checker before codegen — prevents compiling invalid graphs
- Claude's Discretion: whether per-function compilation is supported or whole-program only

### Claude's Discretion
- Debug/release mode design (distinct modes vs single mode with opt flag)
- I/O implementation strategy (direct stdio calls vs runtime library shim)
- Default optimization level when none specified
- Compilation granularity (whole program vs per-function)

</decisions>

<specifics>
## Specific Ideas

- Static linking for self-contained binaries that agents can run and test without environment setup
- Node IDs in runtime errors create a direct feedback loop: agent runs binary, gets error with node ID, queries that node, fixes it
- Cross-compilation via target triple gives agents flexibility to build for deployment targets different from the dev machine

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 05-llvm-compilation-pipeline*
*Context gathered: 2026-02-18*

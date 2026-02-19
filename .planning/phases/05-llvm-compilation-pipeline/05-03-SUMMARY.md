---
phase: 05-llvm-compilation-pipeline
plan: 03
subsystem: codegen
tags: [llvm, inkwell, compiler, pipeline, linker, cli, clap, http, axum]

# Dependency graph
requires:
  - phase: 05-01
    provides: "Type mapping, runtime guards, linker, error types"
  - phase: 05-02
    provides: "Per-function code generation (compile_function) for all ops"
provides:
  - "Top-level compile() orchestrating full pipeline: type check -> codegen -> optimize -> link"
  - "Function-scoped Context pattern (EXEC-04): LLVM Context created and dropped within compile()"
  - "compile_to_ir() variant for testing/debugging without linking"
  - "Main wrapper generator with entry function auto-detection"
  - "POST /programs/{id}/compile HTTP endpoint with CompileRequest/CompileResponse"
  - "'lmlang compile' CLI subcommand with all compilation flags"
  - "Cross-compilation via target triple parameter"
  - "Optimization levels O0-O3 via Module::run_passes (New Pass Manager)"
affects: [05-04]

# Tech tracking
tech-stack:
  added: [clap 4 (derive), lmlang-cli crate]
  patterns: ["function-scoped LLVM Context isolation (EXEC-04)", "dual entry points (HTTP + CLI) sharing same compile() pipeline", "main wrapper generation with entry function auto-detection"]

key-files:
  created:
    - crates/lmlang-codegen/src/compiler.rs
    - crates/lmlang-server/src/handlers/compile.rs
    - crates/lmlang-server/src/schema/compile.rs
    - crates/lmlang-cli/Cargo.toml
    - crates/lmlang-cli/src/main.rs
  modified:
    - crates/lmlang-codegen/src/lib.rs
    - crates/lmlang-server/src/handlers/mod.rs
    - crates/lmlang-server/src/schema/mod.rs
    - crates/lmlang-server/src/service.rs
    - crates/lmlang-server/src/router.rs
    - crates/lmlang-server/Cargo.toml

key-decisions:
  - "Entry function auto-detect: 'main' first, then first public function, then first function"
  - "Entry function must take zero parameters; integer return used as exit code, else return 0"
  - "If entry function is already named 'main', skip wrapper generation"
  - "TypeCheckFailed maps to 422 ValidationFailed in HTTP, exit code 2 in CLI"
  - "CLI outputs CompileResult as JSON to stdout for machine-readable integration"

patterns-established:
  - "compile() + compile_to_ir() dual API: same pipeline, different output for testing"
  - "parse_opt_level() helper shared between server and CLI for consistent opt level parsing"

requirements-completed: [EXEC-03, EXEC-04]

# Metrics
duration: 5min
completed: 2026-02-19
---

# Phase 05 Plan 03: Full Pipeline Compiler with HTTP and CLI Entry Points Summary

**Top-level compile() with function-scoped Context isolation, POST /programs/{id}/compile endpoint, and 'lmlang compile' CLI -- all sharing the same codegen pipeline**

## Performance

- **Duration:** 5 min
- **Started:** 2026-02-19T00:18:23Z
- **Completed:** 2026-02-19T00:23:44Z
- **Tasks:** 3
- **Files modified:** 12

## Accomplishments
- Top-level compile() orchestrating type check -> Context -> codegen -> optimize -> object emit -> link with function-scoped Context
- HTTP endpoint POST /programs/{id}/compile with CompileRequest/CompileResponse matching user-decided fields
- CLI 'lmlang compile' binary with all compilation flags (db, program, opt-level, target, debug-symbols, entry, output-dir)
- Both entry points call the same lmlang_codegen::compile() function per locked user decision

## Task Commits

Each task was committed atomically:

1. **Task 1: Top-level compiler with function-scoped Context pattern** - `b58f951` (feat)
2. **Task 2: HTTP compile endpoint and server integration** - `ec79597` (feat)
3. **Task 3: CLI binary entry point with 'lmlang compile' subcommand** - `6816987` (feat)

## Files Created/Modified
- `crates/lmlang-codegen/src/compiler.rs` - Top-level compile() and compile_to_ir() with main wrapper generator
- `crates/lmlang-codegen/src/lib.rs` - Module registration and re-export of compile/compile_to_ir
- `crates/lmlang-server/src/handlers/compile.rs` - Thin HTTP handler for POST /programs/{id}/compile
- `crates/lmlang-server/src/schema/compile.rs` - CompileRequest and CompileResponse API types
- `crates/lmlang-server/src/service.rs` - ProgramService.compile() method and parse_opt_level helper
- `crates/lmlang-server/src/router.rs` - POST /programs/{id}/compile route registration
- `crates/lmlang-server/src/handlers/mod.rs` - Added compile module
- `crates/lmlang-server/src/schema/mod.rs` - Added compile module
- `crates/lmlang-server/Cargo.toml` - Added lmlang-codegen dependency
- `crates/lmlang-cli/Cargo.toml` - New crate with clap and codegen dependencies
- `crates/lmlang-cli/src/main.rs` - CLI entry point with compile subcommand

## Decisions Made
- Entry function auto-detection: search for "main" first, then first public function, then first function in graph
- Entry function must take zero parameters; if it returns an integer, that becomes the process exit code
- If the entry function is already named "main", skip wrapper generation (avoid symbol conflict)
- TypeCheckFailed from codegen maps to 422 in HTTP (ValidationFailed), exit code 2 in CLI
- CLI outputs CompileResult as JSON to stdout for machine-readable integration by agents and scripts

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed inkwell 0.8 API: try_as_basic_value().basic() not .left()**
- **Found during:** Task 1 (compiler.rs implementation)
- **Issue:** Plan specified `.left()` for extracting basic values from call results, but inkwell 0.8 uses `.basic()` API (ValueKind enum)
- **Fix:** Changed all `.left()` calls to `.basic()` matching the pattern used in codegen.rs
- **Files modified:** crates/lmlang-codegen/src/compiler.rs
- **Verification:** cargo check passes
- **Committed in:** b58f951 (Task 1 commit)

**2. [Rule 3 - Blocking] Added missing Module import in compiler.rs**
- **Found during:** Task 1 (compiler.rs implementation)
- **Issue:** `generate_main_wrapper` parameter used `Module<'ctx>` but `inkwell::module::Module` was not imported
- **Fix:** Added `use inkwell::module::Module;` to imports
- **Files modified:** crates/lmlang-codegen/src/compiler.rs
- **Verification:** cargo check passes
- **Committed in:** b58f951 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes were necessary for compilation. No scope creep.

## Issues Encountered
None beyond the deviations noted above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Full compilation pipeline operational: ProgramGraph -> type check -> LLVM IR -> optimize -> object -> executable
- Both HTTP and CLI entry points share the same pipeline
- Ready for Plan 04 (integration tests verifying end-to-end compilation produces correct output)

---
*Phase: 05-llvm-compilation-pipeline*
*Completed: 2026-02-19*

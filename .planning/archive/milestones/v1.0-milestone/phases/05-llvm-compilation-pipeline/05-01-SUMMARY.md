---
phase: 05-llvm-compilation-pipeline
plan: 01
subsystem: codegen
tags: [inkwell, llvm, type-mapping, runtime-guards, linker, llvm-ir]

# Dependency graph
requires:
  - phase: 01-graph-core
    provides: TypeId, TypeRegistry, LmType, ScalarType, EnumDef, StructDef
  - phase: 03-type-check-interpret
    provides: TypeError for pre-codegen validation errors
provides:
  - lmlang-codegen crate with inkwell 0.8.0 (LLVM 21.1) dependency
  - lm_type_to_llvm mapping for all lmlang types to LLVM IR types
  - CodegenError enum covering all compilation failure modes
  - CompileOptions, CompileResult, OptLevel public API types
  - Runtime function declarations (printf, exit, fprintf, lmlang_runtime_error)
  - Guard helpers for divide-by-zero, overflow, and bounds checking
  - Print op support via typed printf calls
  - System linker integration via cc with platform-specific flags
affects: [05-02, 05-03, 05-04]

# Tech tracking
tech-stack:
  added: [inkwell 0.8.0, llvm-sys 211.0.0]
  patterns: [function-scoped-context, tagged-union-enum-layout, runtime-guard-pattern]

key-files:
  created:
    - crates/lmlang-codegen/Cargo.toml
    - crates/lmlang-codegen/src/lib.rs
    - crates/lmlang-codegen/src/error.rs
    - crates/lmlang-codegen/src/types.rs
    - crates/lmlang-codegen/src/runtime.rs
    - crates/lmlang-codegen/src/linker.rs
    - .cargo/config.toml
  modified: []

key-decisions:
  - "inkwell 0.8.0 with llvm21-1 feature (not 0.7.1/llvm21-0 from research -- version corrected)"
  - "LLVM_SYS_211_PREFIX env var in .cargo/config.toml for build-time LLVM discovery"
  - "Enum tagged union layout: { i32 discriminant, [max_payload_bytes x i8] } with unit-only enums using just { i32 }"
  - "Direct libc calls (printf/fprintf/exit) for I/O rather than separate runtime library"
  - "lmlang_runtime_error emitted as LLVM IR function body with switch on error kind"
  - "Unsigned comparison (ULT) for bounds checking to catch negative indices"

patterns-established:
  - "Tagged union layout: { i32 discriminant, [N x i8] } for enum types"
  - "Guard pattern: compare + conditional branch to error bb + unreachable, builder repositioned to ok bb"
  - "Runtime error via fprintf(stderr, fmt, node_id) + exit(error_kind)"
  - "Platform-specific linking: cfg!(target_os) for macOS (-lSystem) vs Linux (-static)"

requirements-completed: [EXEC-03, EXEC-04]

# Metrics
duration: 7min
completed: 2026-02-18
---

# Phase 5 Plan 1: Codegen Foundation Summary

**lmlang-codegen crate with inkwell LLVM 21 bindings, complete type mapping, runtime error/guard IR emission, and system linker integration via cc**

## Performance

- **Duration:** 7 min
- **Started:** 2026-02-18T23:51:41Z
- **Completed:** 2026-02-18T23:58:39Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Created lmlang-codegen crate compiling against LLVM 21 via inkwell 0.8.0
- Complete type mapping from all lmlang types (Bool, I8-I64, F32, F64, Unit, Array, Struct, Enum, Pointer, Function) to LLVM IR types
- Runtime error function emitted as LLVM IR with switch-based error message selection and fprintf+exit
- Guard helpers (div-by-zero, overflow, bounds check) produce valid LLVM IR verified by module.verify()
- Print op support for all scalar types via printf with proper format strings and type promotion
- System linker module with platform-specific flags ready for executable production

## Task Commits

Each task was committed atomically:

1. **Task 1: Create lmlang-codegen crate with types, error, and public API** - `214ac1f` (feat)
2. **Task 2: Runtime function declarations and system linker** - `6b38961` (feat)

## Files Created/Modified
- `crates/lmlang-codegen/Cargo.toml` - Crate manifest with inkwell llvm21-1 dependency
- `crates/lmlang-codegen/src/lib.rs` - Public API: CompileOptions, CompileResult, OptLevel with serde
- `crates/lmlang-codegen/src/error.rs` - CodegenError enum for all compilation failure modes
- `crates/lmlang-codegen/src/types.rs` - lm_type_to_llvm mapping for all type variants with 15 unit tests
- `crates/lmlang-codegen/src/runtime.rs` - Runtime function declarations, guard helpers, print support with 8 tests
- `crates/lmlang-codegen/src/linker.rs` - link_executable via system cc with 6 tests
- `.cargo/config.toml` - LLVM_SYS_211_PREFIX environment variable for build

## Decisions Made
- Used inkwell 0.8.0 (not 0.7.1 from research) with feature `llvm21-1` (not `llvm21-0`) -- research had incorrect version/feature names
- Set `LLVM_SYS_211_PREFIX` (not `LLVM_SYS_210_PREFIX`) to match llvm-sys 211.0.0
- Enum tagged union uses `{ i32, [N x i8] }` layout; all-unit enums omit payload field for space efficiency
- Direct libc calls (printf/fprintf/exit) for I/O -- simpler than a separate runtime library, sufficient for Phase 5
- `lmlang_runtime_error` emitted as a full LLVM IR function body (not external) -- no separate C runtime needed
- Unsigned comparison (ULT) for bounds checking -- negative indices appear as large unsigned values, caught automatically
- macOS stderr accessed via `__stderrp` global variable (platform-specific)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Corrected inkwell version and feature flag**
- **Found during:** Task 1 (crate creation)
- **Issue:** Plan specified inkwell 0.7.1 with feature `llvm21-0`, but inkwell 0.8.0 is the latest release and the LLVM 21 feature is named `llvm21-1`
- **Fix:** Changed to inkwell 0.8.0 with `llvm21-1` feature; updated LLVM_SYS env var from `_210_` to `_211_`
- **Files modified:** crates/lmlang-codegen/Cargo.toml, .cargo/config.toml
- **Verification:** cargo check compiles successfully
- **Committed in:** 214ac1f (Task 1 commit)

**2. [Rule 3 - Blocking] Added missing BasicType trait import**
- **Found during:** Task 1 (compilation)
- **Issue:** `array_type()` and `fn_type()` methods on `BasicTypeEnum` require the `BasicType` trait to be in scope
- **Fix:** Added `use inkwell::types::BasicType` import
- **Files modified:** crates/lmlang-codegen/src/types.rs
- **Verification:** cargo check passes
- **Committed in:** 214ac1f (Task 1 commit)

**3. [Rule 3 - Blocking] Added indexmap dev-dependency for tests**
- **Found during:** Task 1 (test compilation)
- **Issue:** Tests use `IndexMap` for constructing struct/enum test types but `indexmap` was not in dev-dependencies
- **Fix:** Added `indexmap = "2"` to `[dev-dependencies]`
- **Files modified:** crates/lmlang-codegen/Cargo.toml
- **Verification:** cargo test compiles and passes
- **Committed in:** 214ac1f (Task 1 commit)

---

**Total deviations:** 3 auto-fixed (3 blocking)
**Impact on plan:** All auto-fixes were necessary to unblock compilation. Research had slightly incorrect version information. No scope creep.

## Issues Encountered
None beyond the version/import deviations documented above.

## User Setup Required
None - no external service configuration required. LLVM is pre-installed and the `.cargo/config.toml` configures the build path automatically.

## Next Phase Readiness
- Type mapping, runtime declarations, and linker ready for per-function codegen in Plan 02
- CompileOptions/CompileResult API types ready for compiler integration in Plan 03
- Guard helpers ready for op-level codegen (division, overflow, bounds) in Plan 02

## Self-Check: PASSED

All 8 files verified present. Both task commits (214ac1f, 6b38961) verified in git log.

---
*Phase: 05-llvm-compilation-pipeline*
*Completed: 2026-02-18*

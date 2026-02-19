# Phase 5: LLVM Compilation Pipeline - Research

**Researched:** 2026-02-18
**Domain:** LLVM IR code generation via inkwell, native binary compilation, function-scoped Context isolation
**Confidence:** HIGH

## Summary

Phase 5 compiles computational graphs to native binaries via LLVM/inkwell. The codebase already has a complete op set (~34 operations across Tier 1 and Tier 2), a type system (scalars, structs, arrays, enums, pointers, functions), a work-list interpreter with full control flow, and an HTTP API that can trigger compilation. The compilation pipeline must: (1) lower each op node to LLVM IR instructions, (2) produce native executables via `TargetMachine::write_to_file` + system linker (`cc`), (3) use function-scoped `Context` to avoid lifetime contamination, and (4) guarantee output equivalence with the interpreter for identical inputs.

The primary technical challenge is the graph-to-SSA lowering: the computational graph uses a flat structure with function ownership (each `ComputeNode` has an `owner: FunctionId`) and typed edges (`FlowEdge::Data` with `value_type: TypeId`). Codegen must topologically sort nodes within each function, map types to LLVM types, emit basic blocks for control flow, and handle the interpreter's memory model (Alloc/Store/Load become LLVM `alloca`/`store`/`load`). Runtime error checking (divide-by-zero, overflow, OOB) must be inserted as explicit guard instructions that call an abort function with the originating node ID.

The system has LLVM 21 installed via Homebrew on aarch64-apple-darwin, and inkwell 0.7.1 is the latest crates.io release supporting LLVM 8-21. The new pass manager (`Module::run_passes("default<ON>")`) replaces the legacy `PassManager` for optimization. Linking uses `cc` (Apple Clang 17) to produce standalone executables from `.o` files.

**Primary recommendation:** Create a new `lmlang-codegen` crate with inkwell dependency, implement a `Compiler` struct that takes a `ProgramGraph` reference and produces an executable, integrate via both CLI and HTTP endpoint.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Binary output format:**
- Standalone executable -- single binary that runs directly
- Static linking -- self-contained, no runtime dependencies
- Output placed in deterministic project directory (e.g., `./build/`)
- Debug symbols stripped by default, included only when explicitly requested via flag

**Targets & optimization:**
- Cross-compilation supported -- agent provides LLVM target triple as parameter
- Default target: host machine's native triple
- Optimization via preset levels only (O0/O1/O2/O3) -- no custom LLVM pass configuration

**Runtime I/O & errors:**
- Runtime errors (division by zero, null pointer, OOB) abort with descriptive error message and non-zero exit code
- Error messages include the graph node ID that caused the error -- agent can map back to graph structure for targeted fixes
- Meaningful exit codes: different codes for success (0), runtime error, trap, etc. -- useful for CI and agent inspection

**Compilation trigger:**
- Both HTTP endpoint (POST /programs/{id}/compile) and CLI command (lmlang compile)
- Same underlying pipeline, two entry points -- HTTP for agents, CLI for humans
- Compile response includes: binary path, target triple, binary size, compilation time
- Compilation always runs type checker before codegen -- prevents compiling invalid graphs

### Claude's Discretion
- Debug/release mode design (distinct modes vs single mode with opt flag)
- I/O implementation strategy (direct stdio calls vs runtime library shim)
- Default optimization level when none specified
- Compilation granularity (whole program vs per-function)

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| EXEC-02 | LLVM compilation pipeline maps each op node to LLVM IR instructions via inkwell, handles SSA form, function boundaries, type mapping | Op-to-IR mapping table, type mapping table, function codegen pattern, SSA topological sort |
| EXEC-03 | Compilation produces native binaries (x86_64/ARM) through LLVM optimization passes and system linker | TargetMachine API, `write_to_file`, `cc` linking, cross-compilation via target triple, `Module::run_passes` |
| EXEC-04 | LLVM codegen uses function-scoped Context pattern (create, compile, serialize, drop) to avoid lifetime contamination | Function-scoped Context pattern, Module serialization to bitcode, no LLVM types escaping compilation boundary |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| inkwell | 0.7.1 (feature `llvm21-0`) | Safe Rust wrapper for LLVM C API | Only maintained safe LLVM bindings for Rust; 22k downloads/month |
| llvm-sys | 210.0.0 (via inkwell) | Raw LLVM C API bindings | Transitive dependency of inkwell |
| LLVM | 21.1.8 (installed via Homebrew) | Compiler infrastructure, optimization passes, object code emission | Industry standard; already installed on host |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| clap | 4.x | CLI argument parsing for `lmlang compile` | CLI entry point; already in Rust ecosystem |
| tempfile | 3.x | Temporary object files during compilation | Intermediate `.o` files before linking |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| inkwell | llvm-sys directly | Unsafe, no lifetime safety, more verbose; inkwell wraps it safely |
| cc (system linker) | lld (LLVM linker) | lld is faster but requires separate installation; cc is always available on macOS/Linux |
| Module::run_passes (new PM) | PassManager (legacy PM) | Legacy PM deprecated in LLVM 16+; new PM is the supported path |

**Installation (Cargo.toml for lmlang-codegen):**
```toml
[dependencies]
inkwell = { version = "0.7.1", features = ["llvm21-0"] }
lmlang-core = { path = "../lmlang-core" }
lmlang-check = { path = "../lmlang-check" }
thiserror = "2"
tempfile = "3"
```

**Build requirement:** `LLVM_SYS_210_PREFIX` environment variable must point to LLVM installation (e.g., `/opt/homebrew/opt/llvm`).

## Architecture Patterns

### Recommended Crate Structure
```
crates/lmlang-codegen/
├── Cargo.toml
└── src/
    ├── lib.rs              # Public API: compile(), CompileOptions, CompileResult
    ├── compiler.rs         # Top-level Compiler struct orchestrating the pipeline
    ├── types.rs            # LmType -> LLVM type mapping
    ├── codegen.rs          # Per-function code generation (op -> IR lowering)
    ├── runtime.rs          # Runtime error stubs (abort, print, I/O declarations)
    ├── linker.rs           # Object file -> executable via cc
    └── error.rs            # CodegenError enum
```

### Pattern 1: Function-Scoped Context (EXEC-04)
**What:** Create a fresh LLVM `Context` for each compilation unit, compile all functions into a single `Module` within that Context, serialize the Module to an object file, then drop the Context. No LLVM types, values, or builders escape the function boundary.
**When to use:** Every compilation invocation.
**Why:** inkwell's `'ctx` lifetime ties all IR objects to the Context. If the Context lives too long or is shared across compilations, LLVM types leak and cause lifetime errors. The function-scoped pattern ensures clean isolation.

```rust
// Source: inkwell docs + kaleidoscope example pattern
pub fn compile(graph: &ProgramGraph, options: &CompileOptions) -> Result<CompileResult, CodegenError> {
    // 1. Create fresh context (owns all LLVM IR)
    let context = Context::create();

    // 2. Create module within this context
    let module = context.create_module("lmlang_program");
    let builder = context.create_builder();

    // 3. Declare runtime functions (printf, abort, etc.)
    declare_runtime_functions(&context, &module);

    // 4. Compile each function in the graph
    for (func_id, func_def) in graph.functions() {
        compile_function(&context, &module, &builder, graph, func_id, func_def)?;
    }

    // 5. Generate main() wrapper calling the entry function
    generate_main_wrapper(&context, &module, &builder, graph)?;

    // 6. Run optimization passes
    let target_machine = create_target_machine(&options)?;
    let pass_options = PassBuilderOptions::create();
    let pass_str = match options.opt_level {
        OptLevel::O0 => "default<O0>",
        OptLevel::O1 => "default<O1>",
        OptLevel::O2 => "default<O2>",
        OptLevel::O3 => "default<O3>",
    };
    module.run_passes(pass_str, &target_machine, pass_options)?;

    // 7. Write object file
    let obj_path = temp_dir.path().join("output.o");
    target_machine.write_to_file(&module, FileType::Object, &obj_path)?;

    // 8. Link to executable (Context dropped here, all LLVM types freed)
    link_executable(&obj_path, &options.output_path, &options)?;

    Ok(CompileResult { /* ... */ })
    // Context drops here -- all LLVM IR freed
}
```

### Pattern 2: Type Mapping (LmType -> LLVM Type)
**What:** Convert lmlang TypeIds to inkwell types by looking up in the TypeRegistry and recursively building LLVM types.
**When to use:** At the start of codegen for each function, and when emitting typed operations.

```rust
fn lm_type_to_llvm<'ctx>(
    context: &'ctx Context,
    type_id: TypeId,
    registry: &TypeRegistry,
) -> BasicTypeEnum<'ctx> {
    match type_id {
        TypeId::BOOL => context.bool_type().into(),
        TypeId::I8   => context.i8_type().into(),
        TypeId::I16  => context.i16_type().into(),
        TypeId::I32  => context.i32_type().into(),
        TypeId::I64  => context.i64_type().into(),
        TypeId::F32  => context.f32_type().into(),
        TypeId::F64  => context.f64_type().into(),
        TypeId::UNIT => context.bool_type().into(), // Unit as i1(0)
        _ => {
            // Look up in registry for struct/array/enum/pointer/function
            match registry.get(type_id) {
                Some(LmType::Array { element, length }) => {
                    let elem = lm_type_to_llvm(context, *element, registry);
                    elem.array_type(*length).into()
                }
                Some(LmType::Struct(def)) => {
                    let fields: Vec<BasicTypeEnum> = def.fields.values()
                        .map(|tid| lm_type_to_llvm(context, *tid, registry))
                        .collect();
                    context.struct_type(&fields, false).into()
                }
                Some(LmType::Enum(_)) => {
                    // Tagged union: { i32 discriminant, [max_payload_size x i8] }
                    // Compute max payload size across variants
                    // ...
                }
                Some(LmType::Pointer { .. }) => {
                    context.ptr_type(AddressSpace::default()).into()
                }
                // ...
            }
        }
    }
}
```

### Pattern 3: Per-Function Codegen with Topological Sort
**What:** For each function, collect its nodes, topologically sort them by data dependencies, then emit LLVM IR instructions in order.
**When to use:** Every function compilation.
**Why:** LLVM basic blocks require instructions in order. The graph's data flow edges define a partial order. Topological sorting resolves this into a valid instruction sequence.

```rust
fn compile_function<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    graph: &ProgramGraph,
    func_id: FunctionId,
    func_def: &FunctionDef,
) -> Result<(), CodegenError> {
    // 1. Create LLVM function with correct signature
    let param_types: Vec<BasicMetadataTypeEnum> = func_def.params.iter()
        .map(|(_, tid)| lm_type_to_llvm(context, *tid, &graph.types).into())
        .collect();
    let ret_type = lm_type_to_llvm(context, func_def.return_type, &graph.types);
    let fn_type = ret_type.fn_type(&param_types, false);
    let function = module.add_function(&func_def.name, fn_type, None);

    // 2. Create entry basic block
    let entry_bb = context.append_basic_block(function, "entry");
    builder.position_at_end(entry_bb);

    // 3. Get function nodes and topologically sort by data edges
    let nodes = graph.function_nodes(func_id);
    let sorted = topological_sort(&nodes, graph);

    // 4. Map NodeId -> LLVM Value for SSA tracking
    let mut values: HashMap<NodeId, BasicValueEnum<'ctx>> = HashMap::new();

    // 5. Emit instructions for each node
    for node_id in sorted {
        let node = graph.get_compute_node(node_id).unwrap();
        emit_node(context, module, builder, graph, function,
                  node_id, &node.op, &mut values, &mut basic_blocks)?;
    }

    Ok(())
}
```

### Pattern 4: Runtime Error Guards with Node ID
**What:** Before operations that can fail at runtime (division, array access), emit a guard that checks the condition and calls an abort function with the node ID.
**When to use:** Every division, every dynamic array/struct access, overflow-checked arithmetic.
**Why:** User decision requires error messages to include the graph node ID for agent feedback loop.

```rust
// Declare the runtime error function in the module
fn declare_runtime_functions<'ctx>(context: &'ctx Context, module: &Module<'ctx>) {
    // void lmlang_error(i32 error_kind, i32 node_id)
    let void_type = context.void_type();
    let i32_type = context.i32_type();
    let err_fn_type = void_type.fn_type(&[i32_type.into(), i32_type.into()], false);
    module.add_function("lmlang_runtime_error", err_fn_type, None);

    // i32 lmlang_print_i32(i32 value)  -- for Print op
    let print_i32_type = i32_type.fn_type(&[i32_type.into()], false);
    module.add_function("lmlang_print_i32", print_i32_type, None);
    // ... additional print variants per type
}

// Before sdiv: check divisor != 0
fn emit_div_guard<'ctx>(
    builder: &Builder<'ctx>,
    context: &'ctx Context,
    module: &Module<'ctx>,
    function: FunctionValue<'ctx>,
    divisor: IntValue<'ctx>,
    node_id: NodeId,
) -> Result<(), CodegenError> {
    let zero = divisor.get_type().const_zero();
    let is_zero = builder.build_int_compare(IntPredicate::EQ, divisor, zero, "divzero_check")?;

    let error_bb = context.append_basic_block(function, "divzero_error");
    let continue_bb = context.append_basic_block(function, "divzero_ok");
    builder.build_conditional_branch(is_zero, error_bb, continue_bb)?;

    builder.position_at_end(error_bb);
    let err_fn = module.get_function("lmlang_runtime_error").unwrap();
    let kind = context.i32_type().const_int(1, false);  // 1 = DivideByZero
    let nid = context.i32_type().const_int(node_id.0 as u64, false);
    builder.build_call(err_fn, &[kind.into(), nid.into()], "")?;
    builder.build_unreachable()?;

    builder.position_at_end(continue_bb);
    Ok(())
}
```

### Pattern 5: Main Wrapper Generation
**What:** Generate a `main()` function that calls the program's entry function (conventionally the first public function or a user-specified entry point), converts its return value to an exit code, and returns.
**When to use:** Every standalone binary compilation.

```rust
fn generate_main_wrapper<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    entry_function_name: &str,
) -> Result<(), CodegenError> {
    let i32_type = context.i32_type();
    let main_type = i32_type.fn_type(&[], false);
    let main_fn = module.add_function("main", main_type, None);
    let entry_bb = context.append_basic_block(main_fn, "entry");
    builder.position_at_end(entry_bb);

    // Call the program's entry function
    let entry_fn = module.get_function(entry_function_name)
        .ok_or(CodegenError::NoEntryFunction)?;
    let result = builder.build_call(entry_fn, &[], "result")?;

    // Return 0 for success
    builder.build_return(Some(&i32_type.const_int(0, false)))?;
    Ok(())
}
```

### Pattern 6: Linking via System cc
**What:** Invoke `cc` as a subprocess to link the object file with system libraries.
**When to use:** Final step of every compilation.

```rust
fn link_executable(
    obj_path: &Path,
    output_path: &Path,
    options: &CompileOptions,
) -> Result<(), CodegenError> {
    let mut cmd = std::process::Command::new("cc");
    cmd.arg(obj_path);
    cmd.arg("-o").arg(output_path);

    // Static runtime library (if we compile one)
    // cmd.arg(runtime_lib_path);

    // Strip debug symbols by default
    if !options.debug_symbols {
        cmd.arg("-Wl,-S");  // strip debug symbols on macOS
    }

    // On macOS, -lSystem provides C runtime
    cmd.arg("-lSystem");

    let status = cmd.status()
        .map_err(|e| CodegenError::LinkerFailed(e.to_string()))?;

    if !status.success() {
        return Err(CodegenError::LinkerFailed(
            format!("cc exited with status {}", status)
        ));
    }

    Ok(())
}
```

### Anti-Patterns to Avoid
- **Owning Context in a struct that also borrows from it:** The `Context` must be created at a higher scope and passed by reference into the codegen struct. Self-referential structs don't work in Rust.
- **Using legacy PassManager on LLVM 16+:** The legacy `PassManager`/`PassManagerBuilder` is deprecated. Use `Module::run_passes()` with the new pass manager.
- **Keeping LLVM types across compilations:** Every compilation must create a fresh Context. Never cache LLVM types between compilations.
- **Treating LLVM division as safe:** LLVM considers divide-by-zero as immediate UB. Frontends MUST emit explicit checks before `sdiv`/`udiv`/`srem`/`urem`.
- **Forgetting to initialize LLVM targets:** Must call `Target::initialize_native()` (or `initialize_aarch64()`, `initialize_x86()` for cross-compilation) before creating a TargetMachine.

## Discretion Recommendations

### Debug vs Release Mode
**Recommendation: Single mode with optimization level flag.**
Rationale: The user already decided on O0/O1/O2/O3 preset levels. A separate "debug" mode would just be O0 + debug symbols. Simpler to have `--opt-level O0 --debug-symbols` as independent flags. Default optimization level: **O0** (fastest compilation, easiest debugging, matches interpreter behavior most closely for correctness testing).

### I/O Strategy
**Recommendation: Thin runtime library compiled as a static `.a` archive.**
Rationale: Direct stdio calls via `printf`/`scanf` are fragile (format strings depend on value types, variadic function handling differs across platforms). A thin runtime library with typed functions (`lmlang_print_i32`, `lmlang_print_f64`, `lmlang_print_bool`, `lmlang_readline`, `lmlang_runtime_error`) is more robust, testable, and portable. The runtime library can be written in C (10-20 functions, ~100 lines) and compiled to a `.a` once.

Alternative: Use `printf`/`write` syscalls directly for the initial implementation, defer the runtime library to a later plan if complexity warrants it. Direct stdio is simpler but less clean.

**Recommendation: Start with direct libc calls (printf/fprintf/exit) declared as externals.** Avoid the complexity of compiling and bundling a separate static library in the first iteration. If needed, a runtime library can be added in a follow-up plan.

### Compilation Granularity
**Recommendation: Whole-program compilation only.**
Rationale: Per-function compilation requires incremental compilation infrastructure (dirty tracking, object file caching, incremental linking), which is explicitly deferred to Phase 6 (STORE-05). Whole-program is simpler and sufficient for Phase 5.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| LLVM type creation | Custom LLVM type wrappers | inkwell's `Context::i32_type()`, `struct_type()`, etc. | inkwell handles LLVM ownership safely |
| Optimization passes | Custom optimization pipeline | `Module::run_passes("default<ON>")` | LLVM's standard pipelines are battle-tested |
| Object file emission | Custom object file writer | `TargetMachine::write_to_file(FileType::Object)` | LLVM handles all target-specific details |
| Linking | Custom linker | System `cc` via `std::process::Command` | Handles sysroot, startup code, platform differences |
| Target triple detection | Manual triple construction | `TargetMachine::get_default_triple()` | Always correct for host |
| SSA form | Custom SSA construction | Graph is already SSA-like (each value produced once); use `alloca`/`store`/`load` for mutable variables | LLVM's mem2reg pass promotes allocas to SSA |

**Key insight:** LLVM does the hard work. The codegen layer is a translator from our graph IR to LLVM IR, not a compiler. Let LLVM handle optimization, register allocation, instruction selection, and code emission.

## Common Pitfalls

### Pitfall 1: inkwell Lifetime Contamination
**What goes wrong:** LLVM types from one `Context` are used with a `Module` from a different `Context`, causing segfaults or undefined behavior.
**Why it happens:** inkwell's `'ctx` lifetime parameter prevents this at compile time, but it's easy to accidentally structure code that fights the borrow checker.
**How to avoid:** The `Context` must outlive everything created from it. Create `Context` at the top of `compile()`, create `Module` and `Builder` from it, pass references down. Never store LLVM types in long-lived structures.
**Warning signs:** Fighting the borrow checker when passing LLVM types between functions. Type annotations becoming complex with `'ctx` lifetimes.

### Pitfall 2: Division by Zero is UB in LLVM
**What goes wrong:** Compiled binary crashes with SIGTRAP or produces garbage values on divide-by-zero instead of a clean error message.
**Why it happens:** LLVM IR treats divide-by-zero as immediate undefined behavior. There is no "trap on divide" mode.
**How to avoid:** Emit explicit `icmp eq %divisor, 0` + `br` guard before every `sdiv`/`udiv`/`srem`/`urem`/`fdiv` (for float: check against 0.0). Branch to error handler on zero.
**Warning signs:** Tests passing without divide-by-zero guards (LLVM may optimize away the division, masking the bug).

### Pitfall 3: Integer Overflow Semantics Mismatch
**What goes wrong:** LLVM's `add`/`sub`/`mul` produce wrapping results by default, but the interpreter uses checked arithmetic that traps on overflow.
**Why it happens:** LLVM arithmetic is modular (wrapping) unless you use overflow-checking intrinsics (`llvm.sadd.with.overflow.*`).
**How to avoid:** Use LLVM's overflow intrinsics: `llvm.sadd.with.overflow.i32` etc. These return `{ result, overflow_flag }`. Branch to error handler if overflow_flag is true.
**Warning signs:** Large integer computations producing different results in compiled vs interpreted mode.

### Pitfall 4: Control Flow Lowering Complexity
**What goes wrong:** High-level control flow ops (IfElse, Loop, Match) require multiple basic blocks and phi nodes. Getting the basic block structure wrong causes incorrect code or infinite loops.
**Why it happens:** The graph has both high-level (IfElse, Loop, Match) and low-level (Branch, Jump, Phi) control flow ops. The high-level ops must be lowered to the low-level LLVM equivalents.
**How to avoid:** Handle each control flow pattern as a complete unit. For IfElse: create then_bb, else_bb, merge_bb, emit conditional branch, emit both bodies, emit phi at merge. For Loop: create header_bb, body_bb, exit_bb with phi for loop-carried variables.
**Warning signs:** Unreachable basic blocks in LLVM IR, basic blocks without terminators, phi nodes with wrong predecessor blocks.

### Pitfall 5: LLVM_SYS_PREFIX Not Set
**What goes wrong:** Build fails with "can't find llvm-config" or "LLVM not found".
**Why it happens:** inkwell depends on llvm-sys which needs to find the LLVM installation at build time.
**How to avoid:** Set `LLVM_SYS_210_PREFIX=/opt/homebrew/opt/llvm` in environment or `.cargo/config.toml`. The version number (210) corresponds to LLVM 21.0.
**Warning signs:** Build errors from llvm-sys crate.

### Pitfall 6: Forgetting Target Initialization
**What goes wrong:** `Target::from_triple()` returns `Err` even for valid triples.
**Why it happens:** LLVM lazily initializes target backends. You must call `Target::initialize_native()` or specific initializers before using any target.
**How to avoid:** Call initialization at the start of the compilation pipeline, before any target operations.
**Warning signs:** "No available targets are compatible with triple" errors.

### Pitfall 7: macOS Linker Differences
**What goes wrong:** Linking works on Linux but fails on macOS with missing symbol errors or warnings about deployment targets.
**Why it happens:** macOS uses ld64 (not GNU ld), requires `-lSystem` instead of `-lc`, and has different symbol visibility defaults.
**How to avoid:** Use `cc` (not `ld` directly) for linking -- it abstracts platform differences. Always pass `-lSystem` on macOS.
**Warning signs:** "Undefined symbols" for `_main`, `___stack_chk_fail`, or `_exit`.

## Code Examples

### Complete Op-to-LLVM Mapping Table

#### Tier 1 (Core) Operations

| Op | Inputs | LLVM IR | Notes |
|----|--------|---------|-------|
| `Const { Bool(v) }` | -- | `i1 v` | LLVM bool is i1 |
| `Const { I8(v) }` | -- | `i8 v` | Direct constant |
| `Const { I16(v) }` | -- | `i16 v` | Direct constant |
| `Const { I32(v) }` | -- | `i32 v` | Direct constant |
| `Const { I64(v) }` | -- | `i64 v` | Direct constant |
| `Const { F32(v) }` | -- | `float v` | Narrow f64 storage to f32 |
| `Const { F64(v) }` | -- | `double v` | Direct constant |
| `Const { Unit }` | -- | (no value) | Void/Unit produces no SSA value |
| `BinaryArith { Add }` | lhs, rhs | int: `add`, float: `fadd` | Overflow intrinsic for int |
| `BinaryArith { Sub }` | lhs, rhs | int: `sub`, float: `fsub` | Overflow intrinsic for int |
| `BinaryArith { Mul }` | lhs, rhs | int: `mul`, float: `fmul` | Overflow intrinsic for int |
| `BinaryArith { Div }` | lhs, rhs | int: `sdiv`, float: `fdiv` | Guard: check divisor != 0 |
| `BinaryArith { Rem }` | lhs, rhs | int: `srem`, float: `frem` | Guard: check divisor != 0 |
| `UnaryArith { Neg }` | val | int: `sub 0, %val`, float: `fneg` | |
| `UnaryArith { Abs }` | val | int: `llvm.abs.*`, float: `llvm.fabs.*` | Intrinsic call |
| `Compare { Eq }` | lhs, rhs | int: `icmp eq`, float: `fcmp oeq` | Returns i1 |
| `Compare { Ne }` | lhs, rhs | int: `icmp ne`, float: `fcmp une` | Returns i1 |
| `Compare { Lt }` | lhs, rhs | int: `icmp slt`, float: `fcmp olt` | Signed comparison |
| `Compare { Le }` | lhs, rhs | int: `icmp sle`, float: `fcmp ole` | Signed comparison |
| `Compare { Gt }` | lhs, rhs | int: `icmp sgt`, float: `fcmp ogt` | Signed comparison |
| `Compare { Ge }` | lhs, rhs | int: `icmp sge`, float: `fcmp oge` | Signed comparison |
| `BinaryLogic { And }` | lhs, rhs | `and` | Works for both bool(i1) and int |
| `BinaryLogic { Or }` | lhs, rhs | `or` | Works for both bool(i1) and int |
| `BinaryLogic { Xor }` | lhs, rhs | `xor` | Works for both bool(i1) and int |
| `Not` | val | `xor %val, -1` | All-ones mask for int; `xor %val, true` for i1 |
| `Shift { Shl }` | val, amt | `shl` | Guard: shift amount < bitwidth |
| `Shift { ShrLogical }` | val, amt | `lshr` | Zero-fill shift right |
| `Shift { ShrArith }` | val, amt | `ashr` | Sign-extending shift right |
| `IfElse` | cond (port 0) | `br i1 %cond, label %then, label %else` + merge bb + phi | Full block structure |
| `Loop` | cond (port 0) | `br i1 %cond, label %body, label %exit` in header bb | Loop header + body + exit |
| `Match` | disc (port 0) | `switch i32 %disc, label %default [...]` | One bb per arm + merge |
| `Branch` | cond (port 0) | `br i1 %cond, label %true, label %false` | Low-level conditional branch |
| `Jump` | -- | `br label %target` | Low-level unconditional branch |
| `Phi` | values from predecessors | `phi <ty> [%v1, %bb1], [%v2, %bb2]` | Merge values from branches |
| `Alloc` | -- | `alloca <ty>` | Stack allocation; type from outgoing edge |
| `Load` | ptr (port 0) | `load <ty>, ptr %addr` | Type from outgoing edge |
| `Store` | ptr (port 0), val (port 1) | `store <ty> %val, ptr %addr` | No result value |
| `GetElementPtr` | base (port 0), idx (port 1) | `getelementptr inbounds <ty>, ptr %base, i32 %idx` | Address computation |
| `Call { target }` | args | `call <ret_ty> @<target>(<args>)` | Direct call |
| `IndirectCall` | fn_ptr (port 0), args | `call <ret_ty> %fn_ptr(<args>)` | Through function pointer |
| `Return` | val (port 0) | `ret <ty> %val` or `ret void` | Function terminator |
| `Parameter { index }` | -- | `%arg_N` | LLVM function argument |
| `Print` | val (port 0) | `call @lmlang_print_*(%val)` | Type-specific print function |
| `ReadLine` | -- | `call @lmlang_readline()` | Returns string/buffer |
| `FileOpen` | -- | `call @fopen(...)` | C stdio |
| `FileRead` | -- | `call @fread(...)` | C stdio |
| `FileWrite` | -- | `call @fwrite(...)` | C stdio |
| `FileClose` | -- | `call @fclose(...)` | C stdio |
| `MakeClosure { function }` | captures | alloc env struct + store captures + produce `{ ptr, ptr }` | Environment struct allocation |
| `CaptureAccess { index }` | -- | `getelementptr` on env struct + `load` | Index into closure environment |

#### Tier 2 (Structured) Operations

| Op | Inputs | LLVM IR | Notes |
|----|--------|---------|-------|
| `StructCreate { type_id }` | fields | sequential `insertvalue` | Build struct from field values |
| `StructGet { field_index }` | struct (port 0) | `extractvalue %struct, N` | Extract field by index |
| `StructSet { field_index }` | struct (port 0), val (port 1) | `insertvalue %struct, %val, N` | Functional update |
| `ArrayCreate { length }` | elements | sequential `insertvalue` on `[N x <ty>]` | Build array from elements |
| `ArrayGet` | arr (port 0), idx (port 1) | constant: `extractvalue`, dynamic: alloca + gep + load | Guard: bounds check |
| `ArraySet` | arr (port 0), idx (port 1), val (port 2) | constant: `insertvalue`, dynamic: alloca + gep + store + load | Guard: bounds check |
| `Cast { target_type }` | val (port 0) | `trunc`/`sext`/`fptrunc`/`fpext`/`fptosi`/`sitofp`/etc. | Selected by source+target types |
| `EnumCreate { type_id, variant_index }` | payload (port 0) | store discriminant + bitcast + store payload | Tagged union layout |
| `EnumDiscriminant` | enum (port 0) | `extractvalue %enum, 0` | First field is discriminant |
| `EnumPayload { variant_index }` | enum (port 0) | `extractvalue %enum, 1` + bitcast | Payload is second field |

### inkwell API Quick Reference

```rust
// Types
context.bool_type()                              // -> IntType<'ctx> (i1)
context.i8_type() / i16_type() / i32_type() / i64_type()
context.f32_type() / f64_type()                  // -> FloatType<'ctx>
context.void_type()                              // -> VoidType<'ctx>
context.ptr_type(AddressSpace::default())         // -> PointerType<'ctx>
context.struct_type(&[field_types], packed)       // -> StructType<'ctx>
int_type.array_type(length)                      // -> ArrayType<'ctx>
ret_type.fn_type(&[param_types], is_variadic)    // -> FunctionType<'ctx>

// Module & Function
context.create_module("name")                    // -> Module<'ctx>
module.add_function("name", fn_type, linkage)    // -> FunctionValue<'ctx>
module.get_function("name")                      // -> Option<FunctionValue<'ctx>>

// Builder - Arithmetic
builder.build_int_add(lhs, rhs, "name")?
builder.build_int_sub(lhs, rhs, "name")?
builder.build_int_mul(lhs, rhs, "name")?
builder.build_int_signed_div(lhs, rhs, "name")?
builder.build_int_signed_rem(lhs, rhs, "name")?
builder.build_float_add(lhs, rhs, "name")?
builder.build_float_sub(lhs, rhs, "name")?
builder.build_float_mul(lhs, rhs, "name")?
builder.build_float_div(lhs, rhs, "name")?
builder.build_float_rem(lhs, rhs, "name")?
builder.build_int_neg(val, "name")?              // sub 0, val
builder.build_float_neg(val, "name")?            // fneg

// Builder - Comparison
builder.build_int_compare(IntPredicate::EQ, lhs, rhs, "name")?
builder.build_float_compare(FloatPredicate::OEQ, lhs, rhs, "name")?

// Builder - Logic
builder.build_and(lhs, rhs, "name")?
builder.build_or(lhs, rhs, "name")?
builder.build_xor(lhs, rhs, "name")?
builder.build_not(val, "name")?                  // xor val, -1

// Builder - Shifts
builder.build_left_shift(val, amt, "name")?
builder.build_right_shift(val, amt, sign_extend, "name")?

// Builder - Memory
builder.build_alloca(ty, "name")?                // -> PointerValue
builder.build_load(ty, ptr, "name")?             // -> BasicValueEnum
builder.build_store(ptr, val)?                   // -> InstructionValue
builder.build_struct_gep(ty, ptr, idx, "name")?  // -> PointerValue

// Builder - Control Flow
context.append_basic_block(function, "name")     // -> BasicBlock
builder.position_at_end(basic_block)
builder.build_conditional_branch(cond, then_bb, else_bb)?
builder.build_unconditional_branch(target_bb)?
builder.build_phi(ty, "name")?                   // -> PhiValue
phi.add_incoming(&[(val1, bb1), (val2, bb2)])
builder.build_return(Some(&val))?
builder.build_return(None)?                      // ret void
builder.build_unreachable()?

// Builder - Function Calls
builder.build_call(function, &[args], "name")?
builder.build_indirect_call(fn_type, fn_ptr, &[args], "name")?

// Builder - Casts
builder.build_int_truncate(val, target_int_ty, "name")?
builder.build_int_s_extend(val, target_int_ty, "name")?
builder.build_float_trunc(val, target_float_ty, "name")?
builder.build_float_ext(val, target_float_ty, "name")?
builder.build_float_to_signed_int(val, target_int_ty, "name")?
builder.build_signed_int_to_float(val, target_float_ty, "name")?

// Builder - Struct/Array
builder.build_extract_value(agg, index, "name")?
builder.build_insert_value(agg, val, index, "name")?

// Targets
Target::initialize_native(&InitializationConfig::default())?
Target::initialize_aarch64(&InitializationConfig::default())
Target::initialize_x86(&InitializationConfig::default())
TargetMachine::get_default_triple()              // -> TargetTriple
Target::from_triple(&triple)?                    // -> Target
target.create_target_machine(&triple, cpu, features, opt, reloc, code_model)?
target_machine.write_to_file(&module, FileType::Object, &path)?

// Optimization (New Pass Manager, LLVM 13+)
module.run_passes("default<O2>", &target_machine, PassBuilderOptions::create())?
```

### Exit Code Convention

```
0   = Success (normal program exit)
1   = Runtime error: divide by zero
2   = Runtime error: integer overflow
3   = Runtime error: out of bounds access
4   = Runtime error: null pointer / invalid pointer
5   = Runtime error: type mismatch
100 = Internal compiler error
```

The runtime error function receives `(error_kind: i32, node_id: i32)` and can map the kind to the exit code while printing a descriptive message including the node ID.

### Checked Integer Addition Example (Overflow Intrinsic)

```rust
// Using llvm.sadd.with.overflow.i32
fn emit_checked_add<'ctx>(
    builder: &Builder<'ctx>,
    context: &'ctx Context,
    module: &Module<'ctx>,
    function: FunctionValue<'ctx>,
    lhs: IntValue<'ctx>,
    rhs: IntValue<'ctx>,
    node_id: NodeId,
) -> Result<IntValue<'ctx>, CodegenError> {
    let i32_type = context.i32_type();

    // Declare intrinsic: { i32, i1 } @llvm.sadd.with.overflow.i32(i32, i32)
    let overflow_type = context.struct_type(
        &[i32_type.into(), context.bool_type().into()],
        false,
    );
    let intrinsic_type = overflow_type.fn_type(&[i32_type.into(), i32_type.into()], false);
    let intrinsic = module.add_function("llvm.sadd.with.overflow.i32", intrinsic_type, None);

    // Call intrinsic
    let result = builder.build_call(intrinsic, &[lhs.into(), rhs.into()], "add_result")?;
    let result_struct = result.try_as_basic_value().left().unwrap().into_struct_value();

    // Extract value and overflow flag
    let sum = builder.build_extract_value(result_struct, 0, "sum")?.into_int_value();
    let overflow = builder.build_extract_value(result_struct, 1, "overflow")?.into_int_value();

    // Branch on overflow
    let error_bb = context.append_basic_block(function, "overflow_error");
    let ok_bb = context.append_basic_block(function, "overflow_ok");
    builder.build_conditional_branch(overflow, error_bb, ok_bb)?;

    // Error path
    builder.position_at_end(error_bb);
    let err_fn = module.get_function("lmlang_runtime_error").unwrap();
    let kind = i32_type.const_int(2, false); // 2 = IntegerOverflow
    let nid = i32_type.const_int(node_id.0 as u64, false);
    builder.build_call(err_fn, &[kind.into(), nid.into()], "")?;
    builder.build_unreachable()?;

    // OK path
    builder.position_at_end(ok_bb);
    Ok(sum)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Legacy PassManager | New Pass Manager (`Module::run_passes`) | LLVM 16 (2023) | Legacy PM deprecated; use new PM for optimization |
| Typed pointers (`%T*`) | Opaque pointers (`ptr`) | LLVM 15 (2022) | All pointers are `ptr`; type given at use site (`load i32, ptr`) |
| ORC JIT v1 | ORC JIT v2 | LLVM 12 (2021) | Not relevant for AOT compilation, but noted for completeness |
| inkwell 0.4 | inkwell 0.7.1 | 2025-11 | Supports LLVM 8-21; opaque pointer support added |

**Deprecated/outdated:**
- **Legacy PassManager (`PassManagerBuilder`)**: Deprecated in LLVM 16. Use `Module::run_passes()` instead.
- **Typed pointers**: Removed in LLVM 15. All pointers are opaque (`ptr`). inkwell's `context.ptr_type(AddressSpace::default())` returns opaque pointer.
- **Execution engine for AOT**: JIT execution engine is for interactive/REPL use. For standalone binaries, use `TargetMachine::write_to_file`.

## Open Questions

1. **Overflow intrinsic availability in inkwell**
   - What we know: LLVM has `llvm.sadd.with.overflow.*` family of intrinsics for checked arithmetic
   - What's unclear: Whether inkwell exposes these intrinsics directly or if we need to declare them via `module.add_function("llvm.sadd.with.overflow.i32", ...)`
   - Recommendation: Declare them manually as external functions; LLVM recognizes intrinsic names automatically. Test in first plan.

2. **High-level control flow lowering strategy**
   - What we know: The graph has both high-level (IfElse, Loop, Match) and low-level (Branch, Jump, Phi) control flow ops. Programs may use either style.
   - What's unclear: Whether programs will typically use high-level or low-level control flow, and whether we need to support both simultaneously in codegen
   - Recommendation: Support both. High-level ops get expanded to the same LLVM IR as the low-level equivalents. Test with both styles.

3. **Closure environment allocation**
   - What we know: Closures need an environment struct. MakeClosure stores captures, CaptureAccess loads from them.
   - What's unclear: Whether closure environments should use stack allocation (alloca) or heap allocation (malloc). Stack allocation is simpler but closures may need to outlive their creation scope.
   - Recommendation: Start with stack allocation (alloca). If closures need to escape their scope, use malloc + free. This may need revisiting when real closure-heavy programs are tested.

## Sources

### Primary (HIGH confidence)
- [inkwell GitHub repository](https://github.com/TheDan64/inkwell) - API, examples, version info
- [inkwell docs - Context](https://thedan64.github.io/inkwell/inkwell/context/struct.Context.html) - Type creation methods, module/builder creation
- [inkwell docs - TargetMachine](https://thedan64.github.io/inkwell/inkwell/targets/struct.TargetMachine.html) - write_to_file, cross-compilation, target initialization
- [inkwell crates.io](https://crates.io/crates/inkwell) - Version 0.7.1, LLVM 8-21 support, feature flags
- [Homebrew LLVM formula](https://formulae.brew.sh/formula/llvm) - LLVM 21 installed, /opt/homebrew/opt/llvm
- System verification: `llvm-config --version` = 21.1.8, `rustc --version` = 1.92.0, `cc --version` = Apple clang 17.0.0

### Secondary (MEDIUM confidence)
- [inkwell kaleidoscope example](https://github.com/TheDan64/inkwell/blob/master/examples/kaleidoscope/main.rs) - Verified patterns for Context creation, function compilation, JIT execution
- [Compiler Weekly: LLVM Backend](https://schroer.ca/2021/10/30/cw-llvm-backend/) - Object file emission + linking workflow
- [Create Your Own Programming Language with Rust](https://createlang.rs/01_calculator/basic_llvm.html) - inkwell basic patterns
- [inkwell Discussion #527](https://github.com/TheDan64/inkwell/discussions/527) - Binary generation via write_to_file + linker
- [Mapping High Level Constructs to LLVM IR](https://mapping-high-level-constructs-to-llvm-ir.readthedocs.io/en/latest/basic-constructs/unions.html) - Tagged union layout patterns

### Tertiary (LOW confidence)
- LLVM intrinsic naming convention for overflow detection (needs validation in implementation)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - inkwell is the only viable safe LLVM wrapper for Rust; version verified on crates.io
- Architecture: HIGH - Function-scoped Context pattern verified from multiple sources and inkwell docs; codebase structure well understood from reading all source files
- Pitfalls: HIGH - Division-by-zero UB, lifetime contamination, and overflow semantics are well-documented in LLVM literature
- Op mapping: MEDIUM - Complete table based on LLVM IR spec and inkwell API docs; some intrinsic calls need validation during implementation

**Research date:** 2026-02-18
**Valid until:** 2026-03-18 (stable domain; inkwell release cycle is ~6 months)

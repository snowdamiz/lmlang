//! Top-level compilation pipeline orchestrating the full flow:
//! type check -> Context creation -> function compilation -> optimization
//! -> object emission -> linking.
//!
//! The [`compile`] function is the main entry point. It creates a fresh
//! LLVM [`Context`] that is dropped at function exit, ensuring no LLVM
//! types escape (EXEC-04: function-scoped Context pattern).
//!
//! [`compile_to_ir`] is a variant that returns LLVM IR as a string
//! instead of producing a binary, useful for testing and debugging.

use std::time::Instant;

use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};
use inkwell::types::BasicType;
use inkwell::OptimizationLevel;

use lmlang_check::typecheck;
use lmlang_core::graph::ProgramGraph;

use inkwell::AddressSpace;
use lmlang_core::function::FunctionDef;
use lmlang_core::type_id::TypeId;

use crate::error::CodegenError;
use crate::types::lm_type_to_llvm;
use crate::{codegen, linker, runtime, CompileOptions, CompileResult, OptLevel};

/// Compile a program graph to a native executable.
///
/// Orchestrates the full pipeline:
/// 1. Type check the graph (rejects invalid graphs before codegen)
/// 2. Create a fresh LLVM Context (EXEC-04: function-scoped isolation)
/// 3. Compile each function to LLVM IR
/// 4. Generate main wrapper for the entry function
/// 5. Verify the LLVM module
/// 6. Run optimization passes (O0-O3)
/// 7. Emit object file to temp directory
/// 8. Link into standalone executable
///
/// The Context is created and dropped entirely within this function,
/// so no LLVM types escape the compilation boundary.
pub fn compile(
    graph: &ProgramGraph,
    options: &CompileOptions,
) -> Result<CompileResult, CodegenError> {
    let start = Instant::now();

    // 1. Run type checker -- invalid graphs are rejected before codegen
    let type_errors = typecheck::validate_graph(graph);
    if !type_errors.is_empty() {
        return Err(CodegenError::TypeCheckFailed(type_errors));
    }

    // 2. Create output directory
    std::fs::create_dir_all(&options.output_dir)?;

    // 3. Initialize LLVM targets
    if options.target_triple.is_some() {
        Target::initialize_all(&InitializationConfig::default());
    } else {
        Target::initialize_native(&InitializationConfig::default()).map_err(|e| {
            CodegenError::LlvmError(format!("failed to initialize native target: {}", e))
        })?;
    }

    // 4. Create fresh Context -- this is the EXEC-04 boundary
    let context = Context::create();

    // 5. Create module and builder
    let module = context.create_module("lmlang_program");
    let builder = context.create_builder();

    // 6. Set target triple
    let triple = match &options.target_triple {
        Some(t) => TargetTriple::create(t),
        None => TargetMachine::get_default_triple(),
    };
    module.set_triple(&triple);

    // 7. Declare runtime functions
    runtime::declare_runtime_functions(&context, &module);

    // 8. Forward-declare all function signatures before compiling bodies.
    // This ensures that Call nodes can find their targets regardless of
    // HashMap iteration order (functions may reference each other).
    forward_declare_functions(&context, &module, graph)?;

    // 9. Compile each function in the graph (bodies only -- declarations exist)
    for (func_id, func_def) in graph.functions() {
        codegen::compile_function(&context, &module, &builder, graph, *func_id, func_def)?;
    }

    // 11. Generate main wrapper
    generate_main_wrapper(&context, &module, &builder, graph, options)?;

    // 12. Verify module
    module
        .verify()
        .map_err(|e| CodegenError::LlvmError(format!("module verification failed: {}", e)))?;

    // 13. Create target machine
    let target = Target::from_triple(&triple).map_err(|e| {
        CodegenError::LlvmError(format!("failed to create target from triple: {}", e))
    })?;
    let target_machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            opt_to_llvm(options.opt_level),
            RelocMode::Default,
            CodeModel::Default,
        )
        .ok_or_else(|| CodegenError::LlvmError("failed to create target machine".to_string()))?;

    // 14. Run optimization passes (New Pass Manager)
    let pass_options = PassBuilderOptions::create();
    let pass_str = match options.opt_level {
        OptLevel::O0 => "default<O0>",
        OptLevel::O1 => "default<O1>",
        OptLevel::O2 => "default<O2>",
        OptLevel::O3 => "default<O3>",
    };
    module
        .run_passes(pass_str, &target_machine, pass_options)
        .map_err(|e| CodegenError::LlvmError(format!("optimization passes failed: {}", e)))?;

    // 15. Write object file to temp directory
    let temp_dir = tempfile::tempdir()?;
    let obj_path = temp_dir.path().join("output.o");
    target_machine
        .write_to_file(&module, FileType::Object, &obj_path)
        .map_err(|e| CodegenError::LlvmError(format!("failed to write object file: {}", e)))?;

    // 16. Determine output binary name
    let binary_name = determine_binary_name(graph, options);
    let output_path = options.output_dir.join(&binary_name);

    // 17. Link into executable
    linker::link_executable(&obj_path, &output_path, options.debug_symbols)?;

    // 18. Compute binary size and compilation time
    let binary_size = std::fs::metadata(&output_path)?.len();
    let compilation_time_ms = start.elapsed().as_millis() as u64;
    let target_triple_str = triple.as_str().to_string_lossy().to_string();

    // 19. Context drops here -- all LLVM IR freed, no types escape
    Ok(CompileResult {
        binary_path: output_path,
        target_triple: target_triple_str,
        binary_size,
        compilation_time_ms,
    })
}

/// Compile a program graph to LLVM IR string (for testing/debugging).
///
/// Same pipeline as [`compile`] but returns the LLVM IR text representation
/// instead of producing a binary. Useful for integration tests that want to
/// inspect the generated IR without invoking the linker.
pub fn compile_to_ir(
    graph: &ProgramGraph,
    options: &CompileOptions,
) -> Result<String, CodegenError> {
    // 1. Run type checker
    let type_errors = typecheck::validate_graph(graph);
    if !type_errors.is_empty() {
        return Err(CodegenError::TypeCheckFailed(type_errors));
    }

    // 2. Initialize LLVM targets
    if options.target_triple.is_some() {
        Target::initialize_all(&InitializationConfig::default());
    } else {
        Target::initialize_native(&InitializationConfig::default()).map_err(|e| {
            CodegenError::LlvmError(format!("failed to initialize native target: {}", e))
        })?;
    }

    // 3. Create fresh Context
    let context = Context::create();

    // 4. Create module and builder
    let module = context.create_module("lmlang_program");
    let builder = context.create_builder();

    // 5. Set target triple
    let triple = match &options.target_triple {
        Some(t) => TargetTriple::create(t),
        None => TargetMachine::get_default_triple(),
    };
    module.set_triple(&triple);

    // 6. Declare runtime functions
    runtime::declare_runtime_functions(&context, &module);

    // 7. Forward-declare all function signatures
    forward_declare_functions(&context, &module, graph)?;

    // 8. Compile each function
    for (func_id, func_def) in graph.functions() {
        codegen::compile_function(&context, &module, &builder, graph, *func_id, func_def)?;
    }

    // 9. Generate main wrapper
    generate_main_wrapper(&context, &module, &builder, graph, options)?;

    // 10. Verify module
    module
        .verify()
        .map_err(|e| CodegenError::LlvmError(format!("module verification failed: {}", e)))?;

    // 11. Return IR string
    Ok(module.print_to_string().to_string())
}

/// Compile a program graph incrementally, recompiling only dirty functions.
///
/// Uses [`IncrementalState`] to track per-function hashes and determine which
/// functions need recompilation. Clean functions reuse cached `.o` files.
///
/// On first invocation (empty state), performs a full compilation and populates
/// the cache. On subsequent invocations, only dirty functions and their
/// transitive callers are recompiled.
///
/// If compilation settings change (opt level, target triple, debug flag), the
/// entire cache is invalidated and a full rebuild is performed.
///
/// Returns a tuple of (CompileResult, RecompilationPlan) so the caller can
/// inspect what was recompiled.
pub fn compile_incremental(
    graph: &ProgramGraph,
    options: &CompileOptions,
    state: &mut crate::incremental::IncrementalState,
) -> Result<(CompileResult, crate::incremental::RecompilationPlan), CodegenError> {
    let start = Instant::now();

    // 1. Type check
    let type_errors = typecheck::validate_graph(graph);
    if !type_errors.is_empty() {
        return Err(CodegenError::TypeCheckFailed(type_errors));
    }

    // 2. Create output and cache directories
    std::fs::create_dir_all(&options.output_dir)?;
    std::fs::create_dir_all(state.cache_dir())?;

    // 3. Check if settings changed -- if so, clear hashes (force full rebuild)
    if state.is_settings_changed(options) {
        state.update_hashes(std::collections::HashMap::new());
        state.update_settings_hash(options);
    }

    // 4. Compute current hashes using hash_all_functions_for_compilation
    let blake_hashes = lmlang_storage::hash::hash_all_functions_for_compilation(graph);
    let current_hashes: std::collections::HashMap<lmlang_core::id::FunctionId, [u8; 32]> =
        blake_hashes
            .iter()
            .map(|(&fid, h)| (fid, *h.as_bytes()))
            .collect();

    // 5. Build call graph and compute dirty plan
    let call_graph = crate::incremental::build_call_graph(graph);
    let plan = state.compute_dirty(&current_hashes, &call_graph);

    // 6. Initialize LLVM
    if options.target_triple.is_some() {
        Target::initialize_all(&InitializationConfig::default());
    } else {
        Target::initialize_native(&InitializationConfig::default()).map_err(|e| {
            CodegenError::LlvmError(format!("failed to initialize native target: {}", e))
        })?;
    }

    let triple = match &options.target_triple {
        Some(t) => TargetTriple::create(t),
        None => TargetMachine::get_default_triple(),
    };

    let target = Target::from_triple(&triple).map_err(|e| {
        CodegenError::LlvmError(format!("failed to create target from triple: {}", e))
    })?;

    // 7. Determine which functions need compilation
    let functions_to_compile: Vec<lmlang_core::id::FunctionId> = plan
        .dirty
        .iter()
        .chain(plan.dirty_dependents.iter())
        .copied()
        .collect();

    // 8. Emit runtime module (contains lmlang_runtime_error body)
    {
        let context = Context::create();
        let module = context.create_module("runtime");
        module.set_triple(&triple);

        // Emit the full runtime (with function body)
        runtime::declare_runtime_functions(&context, &module);

        module.verify().map_err(|e| {
            CodegenError::LlvmError(format!("runtime module verification failed: {}", e))
        })?;

        let target_machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                opt_to_llvm(options.opt_level),
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or_else(|| {
                CodegenError::LlvmError("failed to create target machine".to_string())
            })?;

        let runtime_obj = state.cache_dir().join("runtime.o");
        target_machine
            .write_to_file(&module, FileType::Object, &runtime_obj)
            .map_err(|e| {
                CodegenError::LlvmError(format!("failed to write runtime object: {}", e))
            })?;
    }

    // 9. Compile each dirty function to its own .o file
    for &func_id in &functions_to_compile {
        let func_def = graph.get_function(func_id).ok_or_else(|| {
            CodegenError::InvalidGraph(format!("function {} not found", func_id.0))
        })?;

        // Create a fresh Context and Module for this function
        let context = Context::create();
        let module = context.create_module(&format!("func_{}", func_id.0));
        let builder = context.create_builder();
        module.set_triple(&triple);

        // Declare runtime functions as external (body is in runtime.o)
        runtime::declare_runtime_functions_extern(&context, &module);

        // Forward-declare ALL function signatures for cross-module references
        forward_declare_functions(&context, &module, graph)?;

        // Compile only this function's body
        codegen::compile_function(&context, &module, &builder, graph, func_id, func_def)?;

        // If this is the entry function named "main", rename it to __lmlang_main
        // so it doesn't conflict with the main wrapper's @main symbol.
        if func_def.name == "main" {
            if let Some(main_fn) = module.get_function("main") {
                main_fn.as_global_value().set_name("__lmlang_main");
            }
        }

        // Verify the module
        module.verify().map_err(|e| {
            CodegenError::LlvmError(format!(
                "module verification failed for func {}: {}",
                func_id.0, e
            ))
        })?;

        // Create target machine for this function
        let target_machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                opt_to_llvm(options.opt_level),
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or_else(|| {
                CodegenError::LlvmError("failed to create target machine".to_string())
            })?;

        // Run optimization passes
        let pass_options = PassBuilderOptions::create();
        let pass_str = match options.opt_level {
            OptLevel::O0 => "default<O0>",
            OptLevel::O1 => "default<O1>",
            OptLevel::O2 => "default<O2>",
            OptLevel::O3 => "default<O3>",
        };
        module
            .run_passes(pass_str, &target_machine, pass_options)
            .map_err(|e| CodegenError::LlvmError(format!("optimization passes failed: {}", e)))?;

        // Emit to per-function .o file
        let obj_path = state.cached_object_path(func_id);
        target_machine
            .write_to_file(&module, FileType::Object, &obj_path)
            .map_err(|e| CodegenError::LlvmError(format!("failed to write object file: {}", e)))?;
    }

    // 10. Compile main wrapper module
    {
        let context = Context::create();
        let module = context.create_module("main_wrapper");
        let builder = context.create_builder();
        module.set_triple(&triple);

        runtime::declare_runtime_functions_extern(&context, &module);
        forward_declare_functions(&context, &module, graph)?;

        generate_main_wrapper(&context, &module, &builder, graph, options)?;

        module.verify().map_err(|e| {
            CodegenError::LlvmError(format!("main wrapper verification failed: {}", e))
        })?;

        let target_machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                opt_to_llvm(options.opt_level),
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or_else(|| {
                CodegenError::LlvmError("failed to create target machine".to_string())
            })?;

        let pass_options = PassBuilderOptions::create();
        let pass_str = match options.opt_level {
            OptLevel::O0 => "default<O0>",
            OptLevel::O1 => "default<O1>",
            OptLevel::O2 => "default<O2>",
            OptLevel::O3 => "default<O3>",
        };
        module
            .run_passes(pass_str, &target_machine, pass_options)
            .map_err(|e| CodegenError::LlvmError(format!("optimization passes failed: {}", e)))?;

        let main_obj = state.cache_dir().join("main_wrapper.o");
        target_machine
            .write_to_file(&module, FileType::Object, &main_obj)
            .map_err(|e| {
                CodegenError::LlvmError(format!("failed to write main wrapper object: {}", e))
            })?;
    }

    // 11. Collect all .o files (fresh + cached) for linking
    let all_func_ids: Vec<lmlang_core::id::FunctionId> =
        graph.functions().keys().copied().collect();
    let mut obj_paths: Vec<std::path::PathBuf> = Vec::new();
    for &func_id in &all_func_ids {
        obj_paths.push(state.cached_object_path(func_id));
    }
    obj_paths.push(state.cache_dir().join("main_wrapper.o"));
    obj_paths.push(state.cache_dir().join("runtime.o"));

    let obj_refs: Vec<&std::path::Path> = obj_paths.iter().map(|p| p.as_path()).collect();

    // 12. Link into final executable
    let binary_name = determine_binary_name(graph, options);
    let output_path = options.output_dir.join(&binary_name);
    linker::link_objects(&obj_refs, &output_path, options.debug_symbols)?;

    // 13. Update state with new hashes
    state.update_hashes(current_hashes);
    state.update_settings_hash(options);

    let binary_size = std::fs::metadata(&output_path)?.len();
    let compilation_time_ms = start.elapsed().as_millis() as u64;
    let target_triple_str = triple.as_str().to_string_lossy().to_string();

    Ok((
        CompileResult {
            binary_path: output_path,
            target_triple: target_triple_str,
            binary_size,
            compilation_time_ms,
        },
        plan,
    ))
}

/// Generate the `main` wrapper function that calls the program's entry function.
///
/// Entry function selection:
/// 1. If `options.entry_function` is specified, use that name.
/// 2. Otherwise, find the first function named "main".
/// 3. Otherwise, use the first public function.
/// 4. Otherwise, use the first function.
///
/// The entry function must take zero parameters. If it returns an integer type,
/// that value is used as the process exit code; otherwise main returns 0.
fn generate_main_wrapper<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &inkwell::builder::Builder<'ctx>,
    graph: &ProgramGraph,
    options: &CompileOptions,
) -> Result<(), CodegenError> {
    let functions = graph.functions();
    if functions.is_empty() {
        return Err(CodegenError::NoEntryFunction);
    }

    // Find the entry function
    let entry_func_def = if let Some(ref entry_name) = options.entry_function {
        // Explicit entry function name
        functions
            .values()
            .find(|f| f.name == *entry_name)
            .ok_or_else(|| {
                CodegenError::InvalidGraph(format!("entry function '{}' not found", entry_name))
            })?
    } else {
        // Auto-detect: first "main", or first public, or first
        functions
            .values()
            .find(|f| f.name == "main")
            .or_else(|| {
                functions
                    .values()
                    .find(|f| f.visibility == lmlang_core::types::Visibility::Public)
            })
            .or_else(|| functions.values().next())
            .ok_or(CodegenError::NoEntryFunction)?
    };

    // Entry function must take zero parameters
    if !entry_func_def.params.is_empty() {
        return Err(CodegenError::InvalidGraph(format!(
            "entry function '{}' must take zero parameters, but has {}",
            entry_func_def.name,
            entry_func_def.params.len()
        )));
    }

    // Look up the compiled LLVM function
    let entry_llvm_fn = module.get_function(&entry_func_def.name).ok_or_else(|| {
        CodegenError::LlvmError(format!(
            "compiled entry function '{}' not found in LLVM module",
            entry_func_def.name
        ))
    })?;

    // If the entry function is already named "main", rename it to avoid
    // symbol conflicts, then create a proper `i32 @main()` wrapper.
    // This ensures `main` always returns i32 (required by C runtime).
    if entry_func_def.name == "main" {
        entry_llvm_fn.as_global_value().set_name("__lmlang_main");
    }

    // Create main() wrapper: i32 @main()
    let i32_type = context.i32_type();
    let main_fn_type = i32_type.fn_type(&[], false);
    let main_fn = module.add_function("main", main_fn_type, None);
    let entry_bb = context.append_basic_block(main_fn, "entry");
    builder.position_at_end(entry_bb);

    // Call the entry function
    let call_result = builder
        .build_call(entry_llvm_fn, &[], "call_entry")
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    // If entry function returns an integer type, use as exit code
    let return_type = entry_func_def.return_type;
    if return_type == lmlang_core::type_id::TypeId::I32 {
        // Direct i32 return
        let ret_val = call_result.try_as_basic_value().basic().ok_or_else(|| {
            CodegenError::LlvmError("expected return value from entry function".into())
        })?;
        builder
            .build_return(Some(&ret_val))
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
    } else if return_type == lmlang_core::type_id::TypeId::I8
        || return_type == lmlang_core::type_id::TypeId::I16
        || return_type == lmlang_core::type_id::TypeId::I64
    {
        // Truncate or extend to i32 for exit code
        let ret_val = call_result.try_as_basic_value().basic().ok_or_else(|| {
            CodegenError::LlvmError("expected return value from entry function".into())
        })?;
        let int_val = ret_val.into_int_value();
        let bit_width = int_val.get_type().get_bit_width();
        let exit_code: inkwell::values::IntValue<'ctx> = if bit_width < 32 {
            builder
                .build_int_s_extend(int_val, i32_type, "sext_exit")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?
        } else {
            builder
                .build_int_truncate(int_val, i32_type, "trunc_exit")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?
        };
        builder
            .build_return(Some(&exit_code))
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
    } else {
        // Non-integer return (or Unit/void): return 0
        let zero = i32_type.const_int(0, false);
        builder
            .build_return(Some(&zero))
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
    }

    Ok(())
}

/// Forward-declare all function signatures in the LLVM module.
///
/// This ensures that Call nodes can find their target functions regardless of
/// HashMap iteration order. Functions are declared (signature only) before any
/// bodies are compiled.
fn forward_declare_functions<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    graph: &ProgramGraph,
) -> Result<(), CodegenError> {
    let registry = &graph.types;

    for func_def in graph.functions().values() {
        // Skip if already declared (shouldn't happen, but be safe)
        if module.get_function(&func_def.name).is_some() {
            continue;
        }

        let fn_type = build_fn_type(context, func_def, registry)?;
        module.add_function(&func_def.name, fn_type, None);
    }

    Ok(())
}

/// Build the LLVM function type for a FunctionDef.
fn build_fn_type<'ctx>(
    context: &'ctx Context,
    func_def: &FunctionDef,
    registry: &lmlang_core::type_id::TypeRegistry,
) -> Result<inkwell::types::FunctionType<'ctx>, CodegenError> {
    let param_types: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = func_def
        .params
        .iter()
        .map(|(_, tid)| lm_type_to_llvm(context, *tid, registry).map(|t| t.into()))
        .collect::<Result<Vec<_>, _>>()?;

    let mut all_params = param_types;
    if func_def.is_closure && !func_def.captures.is_empty() {
        all_params.push(context.ptr_type(AddressSpace::default()).into());
    }

    let fn_type = if func_def.return_type == TypeId::UNIT {
        context.void_type().fn_type(&all_params, false)
    } else {
        let ret_type = lm_type_to_llvm(context, func_def.return_type, registry)?;
        ret_type.fn_type(&all_params, false)
    };

    Ok(fn_type)
}

/// Map lmlang `OptLevel` to inkwell's `OptimizationLevel`.
fn opt_to_llvm(level: OptLevel) -> OptimizationLevel {
    match level {
        OptLevel::O0 => OptimizationLevel::None,
        OptLevel::O1 => OptimizationLevel::Less,
        OptLevel::O2 => OptimizationLevel::Default,
        OptLevel::O3 => OptimizationLevel::Aggressive,
    }
}

/// Determine the output binary name from the entry function or fallback.
fn determine_binary_name(graph: &ProgramGraph, options: &CompileOptions) -> String {
    if let Some(ref entry_name) = options.entry_function {
        return entry_name.clone();
    }

    // Try to find entry function name
    let functions = graph.functions();
    if let Some(f) = functions.values().find(|f| f.name == "main") {
        return f.name.clone();
    }
    if let Some(f) = functions
        .values()
        .find(|f| f.visibility == lmlang_core::types::Visibility::Public)
    {
        return f.name.clone();
    }
    if let Some(f) = functions.values().next() {
        return f.name.clone();
    }

    "program".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OptLevel;

    #[test]
    fn opt_to_llvm_mapping() {
        assert_eq!(opt_to_llvm(OptLevel::O0), OptimizationLevel::None);
        assert_eq!(opt_to_llvm(OptLevel::O1), OptimizationLevel::Less);
        assert_eq!(opt_to_llvm(OptLevel::O2), OptimizationLevel::Default);
        assert_eq!(opt_to_llvm(OptLevel::O3), OptimizationLevel::Aggressive);
    }

    #[test]
    fn determine_binary_name_defaults_to_program() {
        let graph = ProgramGraph::new("main");
        let options = CompileOptions::default();
        assert_eq!(determine_binary_name(&graph, &options), "program");
    }

    #[test]
    fn determine_binary_name_uses_entry_function_option() {
        let graph = ProgramGraph::new("main");
        let options = CompileOptions {
            entry_function: Some("my_entry".to_string()),
            ..Default::default()
        };
        assert_eq!(determine_binary_name(&graph, &options), "my_entry");
    }
}

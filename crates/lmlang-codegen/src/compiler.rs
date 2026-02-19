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
use inkwell::OptimizationLevel;

use lmlang_check::typecheck;
use lmlang_core::graph::ProgramGraph;

use crate::error::CodegenError;
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
pub fn compile(graph: &ProgramGraph, options: &CompileOptions) -> Result<CompileResult, CodegenError> {
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
        Target::initialize_native(&InitializationConfig::default())
            .map_err(|e| CodegenError::LlvmError(format!("failed to initialize native target: {}", e)))?;
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

    // 8. Compile each function in the graph
    for (func_id, func_def) in graph.functions() {
        codegen::compile_function(&context, &module, &builder, graph, *func_id, func_def)?;
    }

    // 9. Generate main wrapper
    generate_main_wrapper(&context, &module, &builder, graph, options)?;

    // 10. Verify module
    module
        .verify()
        .map_err(|e| CodegenError::LlvmError(format!("module verification failed: {}", e)))?;

    // 11. Create target machine
    let target = Target::from_triple(&triple)
        .map_err(|e| CodegenError::LlvmError(format!("failed to create target from triple: {}", e)))?;
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

    // 12. Run optimization passes (New Pass Manager)
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

    // 13. Write object file to temp directory
    let temp_dir = tempfile::tempdir()?;
    let obj_path = temp_dir.path().join("output.o");
    target_machine
        .write_to_file(&module, FileType::Object, &obj_path)
        .map_err(|e| CodegenError::LlvmError(format!("failed to write object file: {}", e)))?;

    // 14. Determine output binary name
    let binary_name = determine_binary_name(graph, options);
    let output_path = options.output_dir.join(&binary_name);

    // 15. Link into executable
    linker::link_executable(&obj_path, &output_path, options.debug_symbols)?;

    // 16. Compute binary size and compilation time
    let binary_size = std::fs::metadata(&output_path)?.len();
    let compilation_time_ms = start.elapsed().as_millis() as u64;
    let target_triple_str = triple.as_str().to_string_lossy().to_string();

    // 17. Context drops here -- all LLVM IR freed, no types escape
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
pub fn compile_to_ir(graph: &ProgramGraph, options: &CompileOptions) -> Result<String, CodegenError> {
    // 1. Run type checker
    let type_errors = typecheck::validate_graph(graph);
    if !type_errors.is_empty() {
        return Err(CodegenError::TypeCheckFailed(type_errors));
    }

    // 2. Initialize LLVM targets
    if options.target_triple.is_some() {
        Target::initialize_all(&InitializationConfig::default());
    } else {
        Target::initialize_native(&InitializationConfig::default())
            .map_err(|e| CodegenError::LlvmError(format!("failed to initialize native target: {}", e)))?;
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

    // 7. Compile each function
    for (func_id, func_def) in graph.functions() {
        codegen::compile_function(&context, &module, &builder, graph, *func_id, func_def)?;
    }

    // 8. Generate main wrapper
    generate_main_wrapper(&context, &module, &builder, graph, options)?;

    // 9. Verify module
    module
        .verify()
        .map_err(|e| CodegenError::LlvmError(format!("module verification failed: {}", e)))?;

    // 10. Return IR string
    Ok(module.print_to_string().to_string())
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
                CodegenError::InvalidGraph(format!(
                    "entry function '{}' not found",
                    entry_name
                ))
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

    // If the entry function is already named "main", we don't need a wrapper --
    // it IS the main function. But we need to ensure it returns i32.
    // If it's not named "main", create a wrapper.
    if entry_func_def.name == "main" {
        // The function is already named "main" in the module.
        // We trust compile_function set the right signature.
        return Ok(());
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
        let ret_val = call_result
            .try_as_basic_value()
            .basic()
            .ok_or_else(|| CodegenError::LlvmError("expected return value from entry function".into()))?;
        builder
            .build_return(Some(&ret_val))
            .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
    } else if return_type == lmlang_core::type_id::TypeId::I8
        || return_type == lmlang_core::type_id::TypeId::I16
        || return_type == lmlang_core::type_id::TypeId::I64
    {
        // Truncate or extend to i32 for exit code
        let ret_val = call_result
            .try_as_basic_value()
            .basic()
            .ok_or_else(|| CodegenError::LlvmError("expected return value from entry function".into()))?;
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
    if let Some(f) = functions.values().find(|f| f.visibility == lmlang_core::types::Visibility::Public) {
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

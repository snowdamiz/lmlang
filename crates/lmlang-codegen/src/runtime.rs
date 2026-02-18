//! Runtime function declarations for compiled lmlang programs.
//!
//! Declares external C functions (printf, exit, fprintf) and emits
//! the `lmlang_runtime_error` function body in LLVM IR.
//! Also provides guard helpers for division-by-zero, overflow, and
//! bounds checking, plus Print op support via typed printf calls.

use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::values::{BasicMetadataValueEnum, FunctionValue, IntValue};
use inkwell::builder::Builder;
use inkwell::IntPredicate;
use inkwell::AddressSpace;

use crate::error::CodegenError;

/// Runtime error kinds matching the exit code convention.
///
/// These correspond to the `error_kind` parameter passed to
/// `lmlang_runtime_error(i32 error_kind, i32 node_id)`.
pub mod error_kind {
    pub const DIVIDE_BY_ZERO: u64 = 1;
    pub const INTEGER_OVERFLOW: u64 = 2;
    pub const OUT_OF_BOUNDS: u64 = 3;
    pub const NULL_POINTER: u64 = 4;
    pub const TYPE_MISMATCH: u64 = 5;
}

/// Declare all runtime functions in the LLVM module.
///
/// This declares the external C functions that compiled programs call:
/// - `printf(i8*, ...) -> i32` -- variadic C printf for Print op output
/// - `exit(i32) -> void` -- process exit, marked `noreturn`
/// - `fprintf(%struct._IO_FILE*, i8*, ...) -> i32` -- stderr output
///
/// It also emits the `lmlang_runtime_error` function body which
/// calls fprintf(stderr, ...) + exit(error_kind).
pub fn declare_runtime_functions<'ctx>(context: &'ctx Context, module: &Module<'ctx>) {
    let i32_type = context.i32_type();
    let i8_ptr_type = context.ptr_type(AddressSpace::default());
    let void_type = context.void_type();

    // printf(i8*, ...) -> i32
    let printf_type = i32_type.fn_type(&[i8_ptr_type.into()], true);
    module.add_function("printf", printf_type, Some(Linkage::External));

    // exit(i32) -> void
    let exit_type = void_type.fn_type(&[i32_type.into()], false);
    let exit_fn = module.add_function("exit", exit_type, Some(Linkage::External));
    exit_fn.add_attribute(
        inkwell::attributes::AttributeLoc::Function,
        context.create_enum_attribute(
            inkwell::attributes::Attribute::get_named_enum_kind_id("noreturn"),
            0,
        ),
    );

    // fprintf(i8*, i8*, ...) -> i32
    // On macOS/Linux, FILE* is an opaque pointer
    let fprintf_type = i32_type.fn_type(&[i8_ptr_type.into(), i8_ptr_type.into()], true);
    module.add_function("fprintf", fprintf_type, Some(Linkage::External));

    // Declare __stderrp (macOS) or stderr (Linux) as global external
    // On macOS, stderr is accessed via __stderrp global variable
    #[cfg(target_os = "macos")]
    {
        module.add_global(i8_ptr_type, Some(AddressSpace::default()), "__stderrp");
    }
    #[cfg(not(target_os = "macos"))]
    {
        module.add_global(i8_ptr_type, Some(AddressSpace::default()), "stderr");
    }

    // Emit the lmlang_runtime_error function body
    emit_runtime_error_fn(context, module);
}

/// Emit the `lmlang_runtime_error` function body in LLVM IR.
///
/// Takes `(i32 error_kind, i32 node_id)` parameters, builds a switch
/// on error_kind to select an error message string, calls
/// `fprintf(stderr, "Runtime error [kind] at node %d\n", node_id)`,
/// then calls `exit(error_kind)`.
///
/// Error kinds:
/// - 1 = DivideByZero
/// - 2 = IntegerOverflow
/// - 3 = OutOfBounds
/// - 4 = NullPointer
/// - 5 = TypeMismatch
fn emit_runtime_error_fn<'ctx>(context: &'ctx Context, module: &Module<'ctx>) {
    let i32_type = context.i32_type();
    let void_type = context.void_type();
    let i8_ptr_type = context.ptr_type(AddressSpace::default());

    let fn_type = void_type.fn_type(&[i32_type.into(), i32_type.into()], false);
    let function = module.add_function("lmlang_runtime_error", fn_type, None);
    function.add_attribute(
        inkwell::attributes::AttributeLoc::Function,
        context.create_enum_attribute(
            inkwell::attributes::Attribute::get_named_enum_kind_id("noreturn"),
            0,
        ),
    );

    let builder = context.create_builder();
    let entry_bb = context.append_basic_block(function, "entry");
    builder.position_at_end(entry_bb);

    let error_kind = function.get_nth_param(0).unwrap().into_int_value();
    let node_id = function.get_nth_param(1).unwrap().into_int_value();

    // Create error message format strings
    let div_zero_msg = builder
        .build_global_string_ptr("Runtime error: divide by zero at node %d\n", "div_zero_msg")
        .unwrap();
    let overflow_msg = builder
        .build_global_string_ptr(
            "Runtime error: integer overflow at node %d\n",
            "overflow_msg",
        )
        .unwrap();
    let oob_msg = builder
        .build_global_string_ptr(
            "Runtime error: out of bounds access at node %d\n",
            "oob_msg",
        )
        .unwrap();
    let null_ptr_msg = builder
        .build_global_string_ptr("Runtime error: null pointer at node %d\n", "null_ptr_msg")
        .unwrap();
    let type_mismatch_msg = builder
        .build_global_string_ptr(
            "Runtime error: type mismatch at node %d\n",
            "type_mismatch_msg",
        )
        .unwrap();
    let unknown_msg = builder
        .build_global_string_ptr(
            "Runtime error: unknown error (kind %d) at node %d\n",
            "unknown_msg",
        )
        .unwrap();

    // Create basic blocks for each error kind
    let div_zero_bb = context.append_basic_block(function, "div_zero");
    let overflow_bb = context.append_basic_block(function, "overflow");
    let oob_bb = context.append_basic_block(function, "oob");
    let null_ptr_bb = context.append_basic_block(function, "null_ptr");
    let type_mismatch_bb = context.append_basic_block(function, "type_mismatch");
    let default_bb = context.append_basic_block(function, "default");

    // Switch on error_kind
    builder
        .build_switch(
            error_kind,
            default_bb,
            &[
                (i32_type.const_int(error_kind::DIVIDE_BY_ZERO, false), div_zero_bb),
                (i32_type.const_int(error_kind::INTEGER_OVERFLOW, false), overflow_bb),
                (i32_type.const_int(error_kind::OUT_OF_BOUNDS, false), oob_bb),
                (i32_type.const_int(error_kind::NULL_POINTER, false), null_ptr_bb),
                (i32_type.const_int(error_kind::TYPE_MISMATCH, false), type_mismatch_bb),
            ],
        )
        .unwrap();

    // Get stderr and fprintf/exit
    let fprintf_fn = module.get_function("fprintf").unwrap();
    let exit_fn = module.get_function("exit").unwrap();

    #[cfg(target_os = "macos")]
    let stderr_name = "__stderrp";
    #[cfg(not(target_os = "macos"))]
    let stderr_name = "stderr";

    let stderr_global = module.get_global(stderr_name).unwrap();

    // Helper: emit fprintf + exit for a given message and block
    let emit_error_block = |bb: inkwell::basic_block::BasicBlock<'ctx>,
                            msg: inkwell::values::GlobalValue<'ctx>| {
        builder.position_at_end(bb);
        let stderr_val = builder
            .build_load(i8_ptr_type, stderr_global.as_pointer_value(), "stderr")
            .unwrap();
        let msg_ptr = msg.as_pointer_value();
        builder
            .build_call(
                fprintf_fn,
                &[
                    stderr_val.into(),
                    msg_ptr.into(),
                    node_id.into(),
                ],
                "",
            )
            .unwrap();
        builder
            .build_call(exit_fn, &[error_kind.into()], "")
            .unwrap();
        builder.build_unreachable().unwrap();
    };

    emit_error_block(div_zero_bb, div_zero_msg);
    emit_error_block(overflow_bb, overflow_msg);
    emit_error_block(oob_bb, oob_msg);
    emit_error_block(null_ptr_bb, null_ptr_msg);
    emit_error_block(type_mismatch_bb, type_mismatch_msg);

    // Default block: unknown error kind, print both kind and node_id
    builder.position_at_end(default_bb);
    let stderr_val = builder
        .build_load(i8_ptr_type, stderr_global.as_pointer_value(), "stderr")
        .unwrap();
    builder
        .build_call(
            fprintf_fn,
            &[
                stderr_val.into(),
                unknown_msg.as_pointer_value().into(),
                error_kind.into(),
                node_id.into(),
            ],
            "",
        )
        .unwrap();
    builder
        .build_call(exit_fn, &[error_kind.into()], "")
        .unwrap();
    builder.build_unreachable().unwrap();
}

/// Emit a divide-by-zero guard before a division operation.
///
/// Checks that `divisor != 0`. If zero, branches to error block
/// calling `lmlang_runtime_error(1, node_id)`. Otherwise continues
/// to the next instruction.
pub fn emit_div_guard<'ctx>(
    builder: &Builder<'ctx>,
    context: &'ctx Context,
    module: &Module<'ctx>,
    function: FunctionValue<'ctx>,
    divisor: IntValue<'ctx>,
    node_id: u32,
) -> Result<(), CodegenError> {
    let zero = divisor.get_type().const_zero();
    let is_zero = builder
        .build_int_compare(IntPredicate::EQ, divisor, zero, "divzero_check")
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    let error_bb = context.append_basic_block(function, "divzero_error");
    let continue_bb = context.append_basic_block(function, "divzero_ok");
    builder
        .build_conditional_branch(is_zero, error_bb, continue_bb)
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    builder.position_at_end(error_bb);
    let err_fn = module
        .get_function("lmlang_runtime_error")
        .ok_or_else(|| CodegenError::LlvmError("lmlang_runtime_error not found".into()))?;
    let kind = context
        .i32_type()
        .const_int(error_kind::DIVIDE_BY_ZERO, false);
    let nid = context.i32_type().const_int(node_id as u64, false);
    builder
        .build_call(err_fn, &[kind.into(), nid.into()], "")
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
    builder
        .build_unreachable()
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    builder.position_at_end(continue_bb);
    Ok(())
}

/// Emit an integer overflow guard after a checked arithmetic operation.
///
/// Takes the overflow flag from an LLVM overflow intrinsic result.
/// If overflow occurred, branches to error block calling
/// `lmlang_runtime_error(2, node_id)`. Otherwise continues.
pub fn emit_overflow_guard<'ctx>(
    builder: &Builder<'ctx>,
    context: &'ctx Context,
    module: &Module<'ctx>,
    function: FunctionValue<'ctx>,
    overflow_flag: IntValue<'ctx>,
    node_id: u32,
) -> Result<(), CodegenError> {
    let error_bb = context.append_basic_block(function, "overflow_error");
    let continue_bb = context.append_basic_block(function, "overflow_ok");
    builder
        .build_conditional_branch(overflow_flag, error_bb, continue_bb)
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    builder.position_at_end(error_bb);
    let err_fn = module
        .get_function("lmlang_runtime_error")
        .ok_or_else(|| CodegenError::LlvmError("lmlang_runtime_error not found".into()))?;
    let kind = context
        .i32_type()
        .const_int(error_kind::INTEGER_OVERFLOW, false);
    let nid = context.i32_type().const_int(node_id as u64, false);
    builder
        .build_call(err_fn, &[kind.into(), nid.into()], "")
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
    builder
        .build_unreachable()
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    builder.position_at_end(continue_bb);
    Ok(())
}

/// Emit a bounds check guard before an array access.
///
/// Checks that `0 <= index < length`. If out of bounds, branches to
/// error block calling `lmlang_runtime_error(3, node_id)`.
pub fn emit_bounds_guard<'ctx>(
    builder: &Builder<'ctx>,
    context: &'ctx Context,
    module: &Module<'ctx>,
    function: FunctionValue<'ctx>,
    index: IntValue<'ctx>,
    length: IntValue<'ctx>,
    node_id: u32,
) -> Result<(), CodegenError> {
    // Check index < length (unsigned comparison handles negative indices
    // since negative values are large unsigned numbers)
    let in_bounds = builder
        .build_int_compare(IntPredicate::ULT, index, length, "bounds_check")
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    let error_bb = context.append_basic_block(function, "oob_error");
    let continue_bb = context.append_basic_block(function, "oob_ok");
    builder
        .build_conditional_branch(in_bounds, continue_bb, error_bb)
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    builder.position_at_end(error_bb);
    let err_fn = module
        .get_function("lmlang_runtime_error")
        .ok_or_else(|| CodegenError::LlvmError("lmlang_runtime_error not found".into()))?;
    let kind = context
        .i32_type()
        .const_int(error_kind::OUT_OF_BOUNDS, false);
    let nid = context.i32_type().const_int(node_id as u64, false);
    builder
        .build_call(err_fn, &[kind.into(), nid.into()], "")
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
    builder
        .build_unreachable()
        .map_err(|e| CodegenError::LlvmError(e.to_string()))?;

    builder.position_at_end(continue_bb);
    Ok(())
}

/// Emit a printf call to print a value based on its lmlang type.
///
/// Selects the appropriate format string based on the type:
/// - I8/I16/I32 -> `"%d\n"`
/// - I64 -> `"%ld\n"`
/// - F32/F64 -> `"%f\n"`
/// - Bool -> `"true\n"` or `"false\n"` (conditional)
/// - Unit -> (nothing printed)
pub fn emit_print_value<'ctx>(
    builder: &Builder<'ctx>,
    context: &'ctx Context,
    module: &Module<'ctx>,
    value: inkwell::values::BasicValueEnum<'ctx>,
    type_id: lmlang_core::type_id::TypeId,
) -> Result<(), CodegenError> {
    let printf_fn = module
        .get_function("printf")
        .ok_or_else(|| CodegenError::LlvmError("printf not found".into()))?;

    match type_id {
        lmlang_core::type_id::TypeId::BOOL => {
            // For bool, use conditional: print "true\n" or "false\n"
            let true_str = builder
                .build_global_string_ptr("true\n", "true_str")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
            let false_str = builder
                .build_global_string_ptr("false\n", "false_str")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
            let bool_val = value.into_int_value();
            let str_ptr = builder
                .build_select(
                    bool_val,
                    true_str.as_pointer_value(),
                    false_str.as_pointer_value(),
                    "bool_str",
                )
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
            builder
                .build_call(
                    printf_fn,
                    &[BasicMetadataValueEnum::from(
                        str_ptr.into_pointer_value(),
                    )],
                    "",
                )
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
        }
        lmlang_core::type_id::TypeId::I8
        | lmlang_core::type_id::TypeId::I16
        | lmlang_core::type_id::TypeId::I32 => {
            let fmt = builder
                .build_global_string_ptr("%d\n", "int_fmt")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
            // Extend to i32 if narrower
            let int_val = value.into_int_value();
            let i32_val = if int_val.get_type().get_bit_width() < 32 {
                builder
                    .build_int_s_extend(int_val, context.i32_type(), "sext")
                    .map_err(|e| CodegenError::LlvmError(e.to_string()))?
            } else {
                int_val
            };
            builder
                .build_call(
                    printf_fn,
                    &[fmt.as_pointer_value().into(), i32_val.into()],
                    "",
                )
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
        }
        lmlang_core::type_id::TypeId::I64 => {
            let fmt = builder
                .build_global_string_ptr("%ld\n", "long_fmt")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
            builder
                .build_call(
                    printf_fn,
                    &[fmt.as_pointer_value().into(), value.into()],
                    "",
                )
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
        }
        lmlang_core::type_id::TypeId::F32 | lmlang_core::type_id::TypeId::F64 => {
            let fmt = builder
                .build_global_string_ptr("%f\n", "float_fmt")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
            // printf requires double for %f, so extend f32 to f64
            let float_val = value.into_float_value();
            let double_val =
                if float_val.get_type() == context.f32_type() {
                    builder
                        .build_float_ext(float_val, context.f64_type(), "fpext")
                        .map_err(|e| CodegenError::LlvmError(e.to_string()))?
                        .into()
                } else {
                    value
                };
            builder
                .build_call(
                    printf_fn,
                    &[fmt.as_pointer_value().into(), double_val.into()],
                    "",
                )
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
        }
        lmlang_core::type_id::TypeId::UNIT => {
            // Unit: print nothing (or "()\n" for debugging)
        }
        _ => {
            // For unknown types, print a generic representation
            let fmt = builder
                .build_global_string_ptr("<value>\n", "generic_fmt")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
            builder
                .build_call(printf_fn, &[fmt.as_pointer_value().into()], "")
                .map_err(|e| CodegenError::LlvmError(e.to_string()))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::context::Context;

    #[test]
    fn declare_runtime_functions_creates_valid_module() {
        let context = Context::create();
        let module = context.create_module("test_runtime");
        declare_runtime_functions(&context, &module);

        // Verify all expected functions exist
        assert!(module.get_function("printf").is_some());
        assert!(module.get_function("exit").is_some());
        assert!(module.get_function("fprintf").is_some());
        assert!(module.get_function("lmlang_runtime_error").is_some());

        // Verify the module is valid LLVM IR
        assert!(module.verify().is_ok(), "Module verification failed: {:?}", module.verify());
    }

    #[test]
    fn runtime_error_fn_has_correct_signature() {
        let context = Context::create();
        let module = context.create_module("test_sig");
        declare_runtime_functions(&context, &module);

        let err_fn = module.get_function("lmlang_runtime_error").unwrap();
        let fn_type = err_fn.get_type();
        // Should take (i32, i32) -> void
        assert_eq!(fn_type.count_param_types(), 2);
        assert!(fn_type.get_return_type().is_none()); // void return
    }

    #[test]
    fn div_guard_creates_valid_ir() {
        let context = Context::create();
        let module = context.create_module("test_div_guard");
        declare_runtime_functions(&context, &module);

        // Create a test function to host the guard
        let i32_type = context.i32_type();
        let fn_type = i32_type.fn_type(&[i32_type.into()], false);
        let function = module.add_function("test_fn", fn_type, None);
        let entry_bb = context.append_basic_block(function, "entry");

        let builder = context.create_builder();
        builder.position_at_end(entry_bb);

        let divisor = function.get_nth_param(0).unwrap().into_int_value();
        emit_div_guard(&builder, &context, &module, function, divisor, 42).unwrap();

        // After the guard, the builder is positioned in the "ok" block
        // Add a return to make the function complete
        builder
            .build_return(Some(&i32_type.const_int(0, false)))
            .unwrap();

        assert!(module.verify().is_ok(), "Module verification failed: {:?}", module.verify());
    }

    #[test]
    fn overflow_guard_creates_valid_ir() {
        let context = Context::create();
        let module = context.create_module("test_overflow_guard");
        declare_runtime_functions(&context, &module);

        let i32_type = context.i32_type();
        let bool_type = context.bool_type();
        let fn_type = i32_type.fn_type(&[bool_type.into()], false);
        let function = module.add_function("test_fn", fn_type, None);
        let entry_bb = context.append_basic_block(function, "entry");

        let builder = context.create_builder();
        builder.position_at_end(entry_bb);

        let overflow_flag = function.get_nth_param(0).unwrap().into_int_value();
        emit_overflow_guard(&builder, &context, &module, function, overflow_flag, 99).unwrap();

        builder
            .build_return(Some(&i32_type.const_int(0, false)))
            .unwrap();

        assert!(module.verify().is_ok(), "Module verification failed: {:?}", module.verify());
    }

    #[test]
    fn bounds_guard_creates_valid_ir() {
        let context = Context::create();
        let module = context.create_module("test_bounds_guard");
        declare_runtime_functions(&context, &module);

        let i32_type = context.i32_type();
        let fn_type = i32_type.fn_type(&[i32_type.into(), i32_type.into()], false);
        let function = module.add_function("test_fn", fn_type, None);
        let entry_bb = context.append_basic_block(function, "entry");

        let builder = context.create_builder();
        builder.position_at_end(entry_bb);

        let index = function.get_nth_param(0).unwrap().into_int_value();
        let length = function.get_nth_param(1).unwrap().into_int_value();
        emit_bounds_guard(&builder, &context, &module, function, index, length, 7).unwrap();

        builder
            .build_return(Some(&i32_type.const_int(0, false)))
            .unwrap();

        assert!(module.verify().is_ok(), "Module verification failed: {:?}", module.verify());
    }

    #[test]
    fn print_i32_creates_valid_ir() {
        let context = Context::create();
        let module = context.create_module("test_print");
        declare_runtime_functions(&context, &module);

        let i32_type = context.i32_type();
        let fn_type = context.void_type().fn_type(&[i32_type.into()], false);
        let function = module.add_function("test_fn", fn_type, None);
        let entry_bb = context.append_basic_block(function, "entry");

        let builder = context.create_builder();
        builder.position_at_end(entry_bb);

        let value = function.get_nth_param(0).unwrap();
        emit_print_value(
            &builder,
            &context,
            &module,
            value,
            lmlang_core::type_id::TypeId::I32,
        )
        .unwrap();

        builder.build_return(None).unwrap();

        assert!(module.verify().is_ok(), "Module verification failed: {:?}", module.verify());
    }

    #[test]
    fn print_bool_creates_valid_ir() {
        let context = Context::create();
        let module = context.create_module("test_print_bool");
        declare_runtime_functions(&context, &module);

        let bool_type = context.bool_type();
        let fn_type = context.void_type().fn_type(&[bool_type.into()], false);
        let function = module.add_function("test_fn", fn_type, None);
        let entry_bb = context.append_basic_block(function, "entry");

        let builder = context.create_builder();
        builder.position_at_end(entry_bb);

        let value = function.get_nth_param(0).unwrap();
        emit_print_value(
            &builder,
            &context,
            &module,
            value,
            lmlang_core::type_id::TypeId::BOOL,
        )
        .unwrap();

        builder.build_return(None).unwrap();

        assert!(module.verify().is_ok(), "Module verification failed: {:?}", module.verify());
    }

    #[test]
    fn print_f64_creates_valid_ir() {
        let context = Context::create();
        let module = context.create_module("test_print_f64");
        declare_runtime_functions(&context, &module);

        let f64_type = context.f64_type();
        let fn_type = context.void_type().fn_type(&[f64_type.into()], false);
        let function = module.add_function("test_fn", fn_type, None);
        let entry_bb = context.append_basic_block(function, "entry");

        let builder = context.create_builder();
        builder.position_at_end(entry_bb);

        let value = function.get_nth_param(0).unwrap();
        emit_print_value(
            &builder,
            &context,
            &module,
            value,
            lmlang_core::type_id::TypeId::F64,
        )
        .unwrap();

        builder.build_return(None).unwrap();

        assert!(module.verify().is_ok(), "Module verification failed: {:?}", module.verify());
    }
}

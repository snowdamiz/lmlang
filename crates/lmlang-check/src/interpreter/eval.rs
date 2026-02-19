//! Per-op evaluation logic for the graph interpreter.
//!
//! Contains the exhaustive `eval_op` function that maps each [`ComputeNodeOp`]
//! to its runtime evaluation using checked arithmetic and trap semantics.
//!
//! Control flow ops (Branch, IfElse, Loop, Match, Jump, Phi) and function ops
//! (Call, IndirectCall, Return, Parameter) are handled in `state.rs` directly
//! by the Interpreter. This module handles value-producing ops.

use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::NodeId;
use lmlang_core::ops::*;

use super::error::RuntimeError;
use super::value::Value;

/// Evaluates an operation with the given inputs, returning a value (or None).
///
/// This function handles arithmetic, logic, comparison, shifts, constants,
/// structured ops, and closure ops. Control flow and function call/return
/// are handled by the Interpreter's `eval_node` method in `state.rs`.
///
/// # Errors
///
/// Returns `RuntimeError` for:
/// - Integer overflow (checked arithmetic)
/// - Divide by zero
/// - Out of bounds array/struct access
/// - Type mismatches at runtime
pub fn eval_op(
    op: &ComputeNodeOp,
    inputs: &[(u16, Value)],
    node_id: NodeId,
    _graph: &ProgramGraph,
) -> Result<Option<Value>, RuntimeError> {
    match op {
        ComputeNodeOp::Core(core_op) => eval_core_op(core_op, inputs, node_id),
        ComputeNodeOp::Structured(struct_op) => eval_structured_op(struct_op, inputs, node_id),
    }
}

/// Evaluates a Tier 1 (core) operation.
fn eval_core_op(
    op: &ComputeOp,
    inputs: &[(u16, Value)],
    node_id: NodeId,
) -> Result<Option<Value>, RuntimeError> {
    match op {
        ComputeOp::Const { value } => Ok(Some(Value::from_const(value))),

        ComputeOp::BinaryArith { op: arith_op } => {
            let lhs = get_input(inputs, 0, node_id)?;
            let rhs = get_input(inputs, 1, node_id)?;
            let lhs = coerce_bool_to_i8(lhs.clone());
            let rhs = coerce_bool_to_i8(rhs.clone());
            Ok(Some(eval_binary_arith(arith_op, &lhs, &rhs, node_id)?))
        }

        ComputeOp::UnaryArith { op: unary_op } => {
            let val = get_input(inputs, 0, node_id)?;
            let val = coerce_bool_to_i8(val.clone());
            Ok(Some(eval_unary_arith(unary_op, &val, node_id)?))
        }

        ComputeOp::Compare { op: cmp_op } => {
            let lhs = get_input(inputs, 0, node_id)?;
            let rhs = get_input(inputs, 1, node_id)?;
            Ok(Some(eval_compare(cmp_op, lhs, rhs, node_id)?))
        }

        ComputeOp::BinaryLogic { op: logic_op } => {
            let lhs = get_input(inputs, 0, node_id)?;
            let rhs = get_input(inputs, 1, node_id)?;
            Ok(Some(eval_binary_logic(logic_op, lhs, rhs, node_id)?))
        }

        ComputeOp::Not => {
            let val = get_input(inputs, 0, node_id)?;
            Ok(Some(eval_not(val, node_id)?))
        }

        ComputeOp::Shift { op: shift_op } => {
            let val = get_input(inputs, 0, node_id)?;
            let amount = get_input(inputs, 1, node_id)?;
            Ok(Some(eval_shift(shift_op, val, amount, node_id)?))
        }

        // Control flow, function, memory, I/O, and closure ops are handled
        // by the Interpreter in state.rs. If they reach here, it's an internal error.
        ComputeOp::IfElse
        | ComputeOp::Loop
        | ComputeOp::Match
        | ComputeOp::Branch
        | ComputeOp::Jump
        | ComputeOp::Phi
        | ComputeOp::Call { .. }
        | ComputeOp::IndirectCall
        | ComputeOp::Return
        | ComputeOp::Parameter { .. }
        | ComputeOp::Alloc
        | ComputeOp::Load
        | ComputeOp::Store
        | ComputeOp::GetElementPtr
        | ComputeOp::Print
        | ComputeOp::ReadLine
        | ComputeOp::FileOpen
        | ComputeOp::FileRead
        | ComputeOp::FileWrite
        | ComputeOp::FileClose
        | ComputeOp::MakeClosure { .. }
        | ComputeOp::CaptureAccess { .. } => Err(RuntimeError::InternalError {
            message: format!("op {:?} should be handled by Interpreter, not eval_op", op),
        }),

        // Contract ops: check nodes evaluated separately by contract checking hooks,
        // not by the normal work-list flow. Return Ok(None) (no output value).
        ComputeOp::Precondition { .. }
        | ComputeOp::Postcondition { .. }
        | ComputeOp::Invariant { .. } => Ok(None),
    }
}

/// Evaluates a Tier 2 (structured) operation.
fn eval_structured_op(
    op: &StructuredOp,
    inputs: &[(u16, Value)],
    node_id: NodeId,
) -> Result<Option<Value>, RuntimeError> {
    match op {
        StructuredOp::StructCreate { .. } => {
            let mut sorted: Vec<(u16, Value)> = inputs.to_vec();
            sorted.sort_by_key(|(p, _)| *p);
            let fields: Vec<Value> = sorted.into_iter().map(|(_, v)| v).collect();
            Ok(Some(Value::Struct(fields)))
        }

        StructuredOp::StructGet { field_index } => {
            let s = get_input(inputs, 0, node_id)?;
            match s {
                Value::Struct(fields) => {
                    let idx = *field_index as usize;
                    if idx >= fields.len() {
                        Err(RuntimeError::OutOfBoundsAccess {
                            node: node_id,
                            index: idx,
                            size: fields.len(),
                        })
                    } else {
                        Ok(Some(fields[idx].clone()))
                    }
                }
                _ => Err(RuntimeError::TypeMismatchAtRuntime {
                    node: node_id,
                    expected: "Struct".into(),
                    got: s.type_name().into(),
                }),
            }
        }

        StructuredOp::StructSet { field_index } => {
            let s = get_input(inputs, 0, node_id)?;
            let new_val = get_input(inputs, 1, node_id)?;
            match s {
                Value::Struct(fields) => {
                    let idx = *field_index as usize;
                    if idx >= fields.len() {
                        Err(RuntimeError::OutOfBoundsAccess {
                            node: node_id,
                            index: idx,
                            size: fields.len(),
                        })
                    } else {
                        let mut new_fields = fields.clone();
                        new_fields[idx] = new_val.clone();
                        Ok(Some(Value::Struct(new_fields)))
                    }
                }
                _ => Err(RuntimeError::TypeMismatchAtRuntime {
                    node: node_id,
                    expected: "Struct".into(),
                    got: s.type_name().into(),
                }),
            }
        }

        StructuredOp::ArrayCreate { length } => {
            let mut sorted: Vec<(u16, Value)> = inputs.to_vec();
            sorted.sort_by_key(|(p, _)| *p);
            let elements: Vec<Value> = sorted.into_iter().map(|(_, v)| v).collect();
            // Verify length matches if elements are provided
            if !elements.is_empty() && elements.len() != *length as usize {
                return Err(RuntimeError::InternalError {
                    message: format!(
                        "ArrayCreate expected {} elements, got {}",
                        length,
                        elements.len()
                    ),
                });
            }
            Ok(Some(Value::Array(elements)))
        }

        StructuredOp::ArrayGet => {
            let arr = get_input(inputs, 0, node_id)?;
            let idx_val = get_input(inputs, 1, node_id)?;
            match arr {
                Value::Array(elements) => {
                    let idx = value_to_usize(idx_val, node_id)?;
                    if idx >= elements.len() {
                        Err(RuntimeError::OutOfBoundsAccess {
                            node: node_id,
                            index: idx,
                            size: elements.len(),
                        })
                    } else {
                        Ok(Some(elements[idx].clone()))
                    }
                }
                _ => Err(RuntimeError::TypeMismatchAtRuntime {
                    node: node_id,
                    expected: "Array".into(),
                    got: arr.type_name().into(),
                }),
            }
        }

        StructuredOp::ArraySet => {
            let arr = get_input(inputs, 0, node_id)?;
            let idx_val = get_input(inputs, 1, node_id)?;
            let new_val = get_input(inputs, 2, node_id)?;
            match arr {
                Value::Array(elements) => {
                    let idx = value_to_usize(idx_val, node_id)?;
                    if idx >= elements.len() {
                        Err(RuntimeError::OutOfBoundsAccess {
                            node: node_id,
                            index: idx,
                            size: elements.len(),
                        })
                    } else {
                        let mut new_arr = elements.clone();
                        new_arr[idx] = new_val.clone();
                        Ok(Some(Value::Array(new_arr)))
                    }
                }
                _ => Err(RuntimeError::TypeMismatchAtRuntime {
                    node: node_id,
                    expected: "Array".into(),
                    got: arr.type_name().into(),
                }),
            }
        }

        StructuredOp::Cast { target_type } => {
            let val = get_input(inputs, 0, node_id)?;
            Ok(Some(eval_cast(val, *target_type, node_id)?))
        }

        StructuredOp::EnumCreate { variant_index, .. } => {
            let payload = if inputs.is_empty() {
                Value::Unit
            } else {
                get_input(inputs, 0, node_id)?.clone()
            };
            Ok(Some(Value::Enum {
                variant: *variant_index,
                payload: Box::new(payload),
            }))
        }

        StructuredOp::EnumDiscriminant => {
            let val = get_input(inputs, 0, node_id)?;
            match val {
                Value::Enum { variant, .. } => Ok(Some(Value::I32(*variant as i32))),
                _ => Err(RuntimeError::TypeMismatchAtRuntime {
                    node: node_id,
                    expected: "Enum".into(),
                    got: val.type_name().into(),
                }),
            }
        }

        StructuredOp::EnumPayload { .. } => {
            let val = get_input(inputs, 0, node_id)?;
            match val {
                Value::Enum { payload, .. } => Ok(Some(*payload.clone())),
                _ => Err(RuntimeError::TypeMismatchAtRuntime {
                    node: node_id,
                    expected: "Enum".into(),
                    got: val.type_name().into(),
                }),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Arithmetic evaluation
// ---------------------------------------------------------------------------

/// Coerce Bool to I8 for arithmetic operations.
fn coerce_bool_to_i8(v: Value) -> Value {
    match v {
        Value::Bool(b) => Value::I8(if b { 1 } else { 0 }),
        other => other,
    }
}

/// Macro for checked integer arithmetic across all integer types.
macro_rules! checked_int_arith {
    ($op:ident, $lhs:expr, $rhs:expr, $node_id:expr, $( ($variant:ident, $ty:ty) ),+ ) => {
        match ($lhs, $rhs) {
            $(
                (Value::$variant(a), Value::$variant(b)) => {
                    checked_int_op::<$ty>(*a, *b, ArithOp::$op, $node_id)
                        .map(|v| Value::$variant(v))
                }
            )+
            _ => Err(RuntimeError::TypeMismatchAtRuntime {
                node: $node_id,
                expected: "matching numeric types".into(),
                got: format!("{} and {}", $lhs.type_name(), $rhs.type_name()),
            })
        }
    }
}

fn eval_binary_arith(
    op: &ArithOp,
    lhs: &Value,
    rhs: &Value,
    node_id: NodeId,
) -> Result<Value, RuntimeError> {
    // Handle floats separately (no overflow trapping)
    match (lhs, rhs) {
        (Value::F32(a), Value::F32(b)) => {
            return Ok(Value::F32(match op {
                ArithOp::Add => a + b,
                ArithOp::Sub => a - b,
                ArithOp::Mul => a * b,
                ArithOp::Div => {
                    if *b == 0.0 {
                        return Err(RuntimeError::DivideByZero { node: node_id });
                    }
                    a / b
                }
                ArithOp::Rem => {
                    if *b == 0.0 {
                        return Err(RuntimeError::DivideByZero { node: node_id });
                    }
                    a % b
                }
            }));
        }
        (Value::F64(a), Value::F64(b)) => {
            return Ok(Value::F64(match op {
                ArithOp::Add => a + b,
                ArithOp::Sub => a - b,
                ArithOp::Mul => a * b,
                ArithOp::Div => {
                    if *b == 0.0 {
                        return Err(RuntimeError::DivideByZero { node: node_id });
                    }
                    a / b
                }
                ArithOp::Rem => {
                    if *b == 0.0 {
                        return Err(RuntimeError::DivideByZero { node: node_id });
                    }
                    a % b
                }
            }));
        }
        _ => {}
    }

    // Integer checked arithmetic
    match op {
        ArithOp::Add => {
            checked_int_arith!(
                Add,
                lhs,
                rhs,
                node_id,
                (I8, i8),
                (I16, i16),
                (I32, i32),
                (I64, i64)
            )
        }
        ArithOp::Sub => {
            checked_int_arith!(
                Sub,
                lhs,
                rhs,
                node_id,
                (I8, i8),
                (I16, i16),
                (I32, i32),
                (I64, i64)
            )
        }
        ArithOp::Mul => {
            checked_int_arith!(
                Mul,
                lhs,
                rhs,
                node_id,
                (I8, i8),
                (I16, i16),
                (I32, i32),
                (I64, i64)
            )
        }
        ArithOp::Div => {
            // Check for divide by zero first
            match rhs {
                Value::I8(0) | Value::I16(0) | Value::I32(0) | Value::I64(0) => {
                    return Err(RuntimeError::DivideByZero { node: node_id });
                }
                _ => {}
            }
            checked_int_arith!(
                Div,
                lhs,
                rhs,
                node_id,
                (I8, i8),
                (I16, i16),
                (I32, i32),
                (I64, i64)
            )
        }
        ArithOp::Rem => {
            match rhs {
                Value::I8(0) | Value::I16(0) | Value::I32(0) | Value::I64(0) => {
                    return Err(RuntimeError::DivideByZero { node: node_id });
                }
                _ => {}
            }
            checked_int_arith!(
                Rem,
                lhs,
                rhs,
                node_id,
                (I8, i8),
                (I16, i16),
                (I32, i32),
                (I64, i64)
            )
        }
    }
}

/// Performs a checked integer operation using Rust's checked_* methods.
fn checked_int_op<T>(a: T, b: T, op: ArithOp, node_id: NodeId) -> Result<T, RuntimeError>
where
    T: CheckedArith,
{
    let result = match op {
        ArithOp::Add => a.checked_add(b),
        ArithOp::Sub => a.checked_sub(b),
        ArithOp::Mul => a.checked_mul(b),
        ArithOp::Div => a.checked_div(b),
        ArithOp::Rem => a.checked_rem(b),
    };
    result.ok_or(RuntimeError::IntegerOverflow { node: node_id })
}

/// Trait for checked arithmetic on integer types.
trait CheckedArith: Sized {
    fn checked_add(self, rhs: Self) -> Option<Self>;
    fn checked_sub(self, rhs: Self) -> Option<Self>;
    fn checked_mul(self, rhs: Self) -> Option<Self>;
    fn checked_div(self, rhs: Self) -> Option<Self>;
    fn checked_rem(self, rhs: Self) -> Option<Self>;
}

macro_rules! impl_checked_arith {
    ($($ty:ty),+) => {
        $(
            impl CheckedArith for $ty {
                fn checked_add(self, rhs: Self) -> Option<Self> { self.checked_add(rhs) }
                fn checked_sub(self, rhs: Self) -> Option<Self> { self.checked_sub(rhs) }
                fn checked_mul(self, rhs: Self) -> Option<Self> { self.checked_mul(rhs) }
                fn checked_div(self, rhs: Self) -> Option<Self> { self.checked_div(rhs) }
                fn checked_rem(self, rhs: Self) -> Option<Self> { self.checked_rem(rhs) }
            }
        )+
    }
}

impl_checked_arith!(i8, i16, i32, i64);

fn eval_unary_arith(
    op: &UnaryArithOp,
    val: &Value,
    node_id: NodeId,
) -> Result<Value, RuntimeError> {
    match op {
        UnaryArithOp::Neg => match val {
            Value::I8(v) => v
                .checked_neg()
                .map(Value::I8)
                .ok_or(RuntimeError::IntegerOverflow { node: node_id }),
            Value::I16(v) => v
                .checked_neg()
                .map(Value::I16)
                .ok_or(RuntimeError::IntegerOverflow { node: node_id }),
            Value::I32(v) => v
                .checked_neg()
                .map(Value::I32)
                .ok_or(RuntimeError::IntegerOverflow { node: node_id }),
            Value::I64(v) => v
                .checked_neg()
                .map(Value::I64)
                .ok_or(RuntimeError::IntegerOverflow { node: node_id }),
            Value::F32(v) => Ok(Value::F32(-v)),
            Value::F64(v) => Ok(Value::F64(-v)),
            _ => Err(RuntimeError::TypeMismatchAtRuntime {
                node: node_id,
                expected: "numeric".into(),
                got: val.type_name().into(),
            }),
        },
        UnaryArithOp::Abs => match val {
            Value::I8(v) => v
                .checked_abs()
                .map(Value::I8)
                .ok_or(RuntimeError::IntegerOverflow { node: node_id }),
            Value::I16(v) => v
                .checked_abs()
                .map(Value::I16)
                .ok_or(RuntimeError::IntegerOverflow { node: node_id }),
            Value::I32(v) => v
                .checked_abs()
                .map(Value::I32)
                .ok_or(RuntimeError::IntegerOverflow { node: node_id }),
            Value::I64(v) => v
                .checked_abs()
                .map(Value::I64)
                .ok_or(RuntimeError::IntegerOverflow { node: node_id }),
            Value::F32(v) => Ok(Value::F32(v.abs())),
            Value::F64(v) => Ok(Value::F64(v.abs())),
            _ => Err(RuntimeError::TypeMismatchAtRuntime {
                node: node_id,
                expected: "numeric".into(),
                got: val.type_name().into(),
            }),
        },
    }
}

// ---------------------------------------------------------------------------
// Comparison evaluation
// ---------------------------------------------------------------------------

fn eval_compare(
    op: &CmpOp,
    lhs: &Value,
    rhs: &Value,
    node_id: NodeId,
) -> Result<Value, RuntimeError> {
    macro_rules! cmp {
        ($a:expr, $b:expr, $op:ident) => {
            match $op {
                CmpOp::Eq => $a == $b,
                CmpOp::Ne => $a != $b,
                CmpOp::Lt => $a < $b,
                CmpOp::Le => $a <= $b,
                CmpOp::Gt => $a > $b,
                CmpOp::Ge => $a >= $b,
            }
        };
    }

    let result = match (lhs, rhs) {
        (Value::Bool(a), Value::Bool(b)) => cmp!(a, b, op),
        (Value::I8(a), Value::I8(b)) => cmp!(a, b, op),
        (Value::I16(a), Value::I16(b)) => cmp!(a, b, op),
        (Value::I32(a), Value::I32(b)) => cmp!(a, b, op),
        (Value::I64(a), Value::I64(b)) => cmp!(a, b, op),
        (Value::F32(a), Value::F32(b)) => cmp!(a, b, op),
        (Value::F64(a), Value::F64(b)) => cmp!(a, b, op),
        _ => {
            return Err(RuntimeError::TypeMismatchAtRuntime {
                node: node_id,
                expected: "matching comparable types".into(),
                got: format!("{} and {}", lhs.type_name(), rhs.type_name()),
            })
        }
    };

    Ok(Value::Bool(result))
}

// ---------------------------------------------------------------------------
// Logic evaluation
// ---------------------------------------------------------------------------

fn eval_binary_logic(
    op: &LogicOp,
    lhs: &Value,
    rhs: &Value,
    node_id: NodeId,
) -> Result<Value, RuntimeError> {
    match (lhs, rhs) {
        // Bool: standard logical
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(match op {
            LogicOp::And => *a && *b,
            LogicOp::Or => *a || *b,
            LogicOp::Xor => *a ^ *b,
        })),
        // Integer: bitwise
        (Value::I8(a), Value::I8(b)) => Ok(Value::I8(match op {
            LogicOp::And => a & b,
            LogicOp::Or => a | b,
            LogicOp::Xor => a ^ b,
        })),
        (Value::I16(a), Value::I16(b)) => Ok(Value::I16(match op {
            LogicOp::And => a & b,
            LogicOp::Or => a | b,
            LogicOp::Xor => a ^ b,
        })),
        (Value::I32(a), Value::I32(b)) => Ok(Value::I32(match op {
            LogicOp::And => a & b,
            LogicOp::Or => a | b,
            LogicOp::Xor => a ^ b,
        })),
        (Value::I64(a), Value::I64(b)) => Ok(Value::I64(match op {
            LogicOp::And => a & b,
            LogicOp::Or => a | b,
            LogicOp::Xor => a ^ b,
        })),
        _ => Err(RuntimeError::TypeMismatchAtRuntime {
            node: node_id,
            expected: "Bool or matching integer types".into(),
            got: format!("{} and {}", lhs.type_name(), rhs.type_name()),
        }),
    }
}

fn eval_not(val: &Value, node_id: NodeId) -> Result<Value, RuntimeError> {
    match val {
        Value::Bool(b) => Ok(Value::Bool(!b)),
        Value::I8(v) => Ok(Value::I8(!v)),
        Value::I16(v) => Ok(Value::I16(!v)),
        Value::I32(v) => Ok(Value::I32(!v)),
        Value::I64(v) => Ok(Value::I64(!v)),
        _ => Err(RuntimeError::TypeMismatchAtRuntime {
            node: node_id,
            expected: "Bool or integer".into(),
            got: val.type_name().into(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Shift evaluation
// ---------------------------------------------------------------------------

fn eval_shift(
    op: &ShiftOp,
    val: &Value,
    amount: &Value,
    node_id: NodeId,
) -> Result<Value, RuntimeError> {
    let shift = value_to_u32(amount, node_id)?;

    macro_rules! do_shift {
        ($v:expr, $ty:ty, $bits:expr) => {{
            if shift >= $bits {
                return Err(RuntimeError::IntegerOverflow { node: node_id });
            }
            match op {
                ShiftOp::Shl => Ok(Value::from_int::<$ty>(($v << shift) as $ty)),
                ShiftOp::ShrLogical => {
                    // Logical shift right: treat as unsigned
                    let unsigned = $v as $ty;
                    Ok(Value::from_int::<$ty>((unsigned >> shift) as $ty))
                }
                ShiftOp::ShrArith => {
                    // Arithmetic shift right: preserves sign
                    Ok(Value::from_int::<$ty>(($v >> shift) as $ty))
                }
            }
        }};
    }

    match val {
        Value::I8(v) => do_shift!(*v, i8, 8),
        Value::I16(v) => do_shift!(*v, i16, 16),
        Value::I32(v) => do_shift!(*v, i32, 32),
        Value::I64(v) => do_shift!(*v, i64, 64),
        _ => Err(RuntimeError::TypeMismatchAtRuntime {
            node: node_id,
            expected: "integer".into(),
            got: val.type_name().into(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Cast evaluation
// ---------------------------------------------------------------------------

fn eval_cast(
    val: &Value,
    target_type: lmlang_core::type_id::TypeId,
    node_id: NodeId,
) -> Result<Value, RuntimeError> {
    use lmlang_core::type_id::TypeId;

    // Extract source as i64 or f64 for conversion
    match target_type {
        TypeId::BOOL => match val {
            Value::I8(v) => Ok(Value::Bool(*v != 0)),
            Value::I16(v) => Ok(Value::Bool(*v != 0)),
            Value::I32(v) => Ok(Value::Bool(*v != 0)),
            Value::I64(v) => Ok(Value::Bool(*v != 0)),
            Value::Bool(b) => Ok(Value::Bool(*b)),
            _ => Err(RuntimeError::TypeMismatchAtRuntime {
                node: node_id,
                expected: "numeric or bool for cast to Bool".into(),
                got: val.type_name().into(),
            }),
        },
        TypeId::I8 => Ok(Value::I8(value_to_i64(val, node_id)? as i8)),
        TypeId::I16 => Ok(Value::I16(value_to_i64(val, node_id)? as i16)),
        TypeId::I32 => Ok(Value::I32(value_to_i64(val, node_id)? as i32)),
        TypeId::I64 => Ok(Value::I64(value_to_i64(val, node_id)?)),
        TypeId::F32 => Ok(Value::F32(value_to_f64(val, node_id)? as f32)),
        TypeId::F64 => Ok(Value::F64(value_to_f64(val, node_id)?)),
        _ => {
            // For non-scalar target types, just pass through
            Ok(val.clone())
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Gets an input value by port number.
fn get_input(inputs: &[(u16, Value)], port: u16, node_id: NodeId) -> Result<&Value, RuntimeError> {
    inputs
        .iter()
        .find(|(p, _)| *p == port)
        .map(|(_, v)| v)
        .ok_or(RuntimeError::MissingValue {
            node: node_id,
            port,
        })
}

/// Converts a Value to usize for indexing.
fn value_to_usize(v: &Value, node_id: NodeId) -> Result<usize, RuntimeError> {
    match v {
        Value::I8(n) => Ok(*n as usize),
        Value::I16(n) => Ok(*n as usize),
        Value::I32(n) => Ok(*n as usize),
        Value::I64(n) => Ok(*n as usize),
        _ => Err(RuntimeError::TypeMismatchAtRuntime {
            node: node_id,
            expected: "integer".into(),
            got: v.type_name().into(),
        }),
    }
}

/// Converts a Value to u32 for shift amounts.
fn value_to_u32(v: &Value, node_id: NodeId) -> Result<u32, RuntimeError> {
    match v {
        Value::I8(n) => Ok(*n as u32),
        Value::I16(n) => Ok(*n as u32),
        Value::I32(n) => Ok(*n as u32),
        Value::I64(n) => Ok(*n as u32),
        _ => Err(RuntimeError::TypeMismatchAtRuntime {
            node: node_id,
            expected: "integer".into(),
            got: v.type_name().into(),
        }),
    }
}

/// Converts a Value to i64 for casting.
fn value_to_i64(v: &Value, node_id: NodeId) -> Result<i64, RuntimeError> {
    match v {
        Value::Bool(b) => Ok(if *b { 1 } else { 0 }),
        Value::I8(n) => Ok(*n as i64),
        Value::I16(n) => Ok(*n as i64),
        Value::I32(n) => Ok(*n as i64),
        Value::I64(n) => Ok(*n),
        Value::F32(n) => Ok(*n as i64),
        Value::F64(n) => Ok(*n as i64),
        _ => Err(RuntimeError::TypeMismatchAtRuntime {
            node: node_id,
            expected: "numeric".into(),
            got: v.type_name().into(),
        }),
    }
}

/// Converts a Value to f64 for casting.
fn value_to_f64(v: &Value, node_id: NodeId) -> Result<f64, RuntimeError> {
    match v {
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::I8(n) => Ok(*n as f64),
        Value::I16(n) => Ok(*n as f64),
        Value::I32(n) => Ok(*n as f64),
        Value::I64(n) => Ok(*n as f64),
        Value::F32(n) => Ok(*n as f64),
        Value::F64(n) => Ok(*n),
        _ => Err(RuntimeError::TypeMismatchAtRuntime {
            node: node_id,
            expected: "numeric".into(),
            got: v.type_name().into(),
        }),
    }
}

/// Helper trait to create Value from integer types.
impl Value {
    fn from_int<T>(v: T) -> Value
    where
        T: Into<ValueInt>,
    {
        v.into().0
    }
}

struct ValueInt(Value);

impl From<i8> for ValueInt {
    fn from(v: i8) -> Self {
        ValueInt(Value::I8(v))
    }
}

impl From<i16> for ValueInt {
    fn from(v: i16) -> Self {
        ValueInt(Value::I16(v))
    }
}

impl From<i32> for ValueInt {
    fn from(v: i32) -> Self {
        ValueInt(Value::I32(v))
    }
}

impl From<i64> for ValueInt {
    fn from(v: i64) -> Self {
        ValueInt(Value::I64(v))
    }
}

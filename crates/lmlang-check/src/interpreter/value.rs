//! Runtime value representation for the graph interpreter.
//!
//! [`Value`] is the dynamic runtime counterpart to lmlang-core's static type
//! system. Every node evaluation produces a `Value` that flows through data
//! edges to downstream nodes.

use lmlang_core::id::FunctionId;
use lmlang_core::type_id::TypeId;
use lmlang_core::types::ConstValue;
use serde::{Deserialize, Serialize};

/// A runtime value produced or consumed by interpreter node evaluation.
///
/// Maps to the lmlang type system:
/// - Scalars: `Bool`, `I8`-`I64`, `F32`, `F64`
/// - Compound: `Array`, `Struct`, `Enum`
/// - Special: `Unit`, `Pointer`, `FunctionRef`, `Closure`
///
/// Note: `F32` stores an actual `f32` at runtime, unlike `ConstValue::F32`
/// which stores f64 for derive safety. The conversion happens in
/// [`Value::from_const`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Bool(bool),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Unit,
    Array(Vec<Value>),
    /// Struct fields in declaration order.
    Struct(Vec<Value>),
    Enum {
        variant: u32,
        payload: Box<Value>,
    },
    /// Index into interpreter memory.
    Pointer(usize),
    FunctionRef(FunctionId),
    Closure {
        function: FunctionId,
        captures: Vec<Value>,
    },
}

impl Value {
    /// Converts a compile-time [`ConstValue`] to a runtime [`Value`].
    ///
    /// Note: `ConstValue::F32(f64_bits)` converts to `Value::F32(bits as f32)`
    /// per the Phase 1 decision that F32 constants are stored as f64 internally.
    pub fn from_const(cv: &ConstValue) -> Value {
        match cv {
            ConstValue::Bool(b) => Value::Bool(*b),
            ConstValue::I8(v) => Value::I8(*v),
            ConstValue::I16(v) => Value::I16(*v),
            ConstValue::I32(v) => Value::I32(*v),
            ConstValue::I64(v) => Value::I64(*v),
            ConstValue::F32(bits) => Value::F32(*bits as f32),
            ConstValue::F64(v) => Value::F64(*v),
            ConstValue::Unit => Value::Unit,
        }
    }

    /// Returns the [`TypeId`] of this runtime value based on its variant.
    ///
    /// For scalar and unit types, returns the well-known built-in TypeId.
    /// For compound types, returns a placeholder (the actual TypeId tracking
    /// is handled by the type checker at the graph level, not at runtime).
    pub fn type_id(&self) -> TypeId {
        match self {
            Value::Bool(_) => TypeId::BOOL,
            Value::I8(_) => TypeId::I8,
            Value::I16(_) => TypeId::I16,
            Value::I32(_) => TypeId::I32,
            Value::I64(_) => TypeId::I64,
            Value::F32(_) => TypeId::F32,
            Value::F64(_) => TypeId::F64,
            Value::Unit => TypeId::UNIT,
            // Compound types don't have a static TypeId available at runtime
            // without the registry. Return UNIT as a placeholder -- the type
            // checker ensures correctness at the graph level.
            Value::Array(_) => TypeId::UNIT,
            Value::Struct(_) => TypeId::UNIT,
            Value::Enum { .. } => TypeId::UNIT,
            Value::Pointer(_) => TypeId::UNIT,
            Value::FunctionRef(_) => TypeId::UNIT,
            Value::Closure { .. } => TypeId::UNIT,
        }
    }

    /// Returns a human-readable description of the value's type.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "Bool",
            Value::I8(_) => "I8",
            Value::I16(_) => "I16",
            Value::I32(_) => "I32",
            Value::I64(_) => "I64",
            Value::F32(_) => "F32",
            Value::F64(_) => "F64",
            Value::Unit => "Unit",
            Value::Array(_) => "Array",
            Value::Struct(_) => "Struct",
            Value::Enum { .. } => "Enum",
            Value::Pointer(_) => "Pointer",
            Value::FunctionRef(_) => "FunctionRef",
            Value::Closure { .. } => "Closure",
        }
    }
}

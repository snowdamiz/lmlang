//! The lmlang type system.
//!
//! Provides the complete set of types used in lmlang programs:
//! scalars (Bool, I8-I64, F32, F64), arrays, structs, enums/tagged unions,
//! pointers, function signatures, Unit, and Never.
//!
//! All types use nominal identity via [`TypeId`]. Structs and enums use
//! [`IndexMap`] for insertion-ordered fields/variants.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::id::ModuleId;
use crate::type_id::TypeId;

/// The lmlang type system. Each variant represents a distinct kind of type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LmType {
    /// Scalar types mapping directly to LLVM primitives.
    Scalar(ScalarType),

    /// Fixed-size array: `[T; N]`.
    Array { element: TypeId, length: u32 },

    /// Named struct with ordered fields (nominal typing).
    Struct(StructDef),

    /// Named enum / tagged union (nominal typing).
    Enum(EnumDef),

    /// Pointer/reference to another type.
    Pointer { pointee: TypeId, mutable: bool },

    /// Function signature.
    Function {
        params: Vec<TypeId>,
        return_type: TypeId,
    },

    /// Unit type (zero-size, like Rust's `()`).
    Unit,

    /// Never type (diverging, like Rust's `!`).
    Never,
}

/// Scalar (primitive) types with direct LLVM mapping.
///
/// No unsigned integers -- follows the LLVM approach where signedness
/// is determined at the operation level (sdiv vs udiv, sext vs zext),
/// not at the type level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScalarType {
    Bool,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

/// Named struct definition with insertion-ordered fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructDef {
    pub name: String,
    pub type_id: TypeId,
    pub fields: IndexMap<String, TypeId>,
    pub module: ModuleId,
    pub visibility: Visibility,
}

/// Named enum (tagged union) definition with insertion-ordered variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumDef {
    pub name: String,
    pub type_id: TypeId,
    pub variants: IndexMap<String, EnumVariant>,
    pub module: ModuleId,
    pub visibility: Visibility,
}

/// A single variant within an enum definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    /// Index of this variant (used as discriminant).
    pub index: u32,
    /// Payload type, if any (`None` = unit variant).
    pub payload: Option<TypeId>,
}

/// Visibility of a type or function across module boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
}

/// Constant literal values used by `Const` op nodes.
///
/// Note: `F32` stores its value as `f64` internally. This is because `f32`
/// does not implement `Eq` in Rust (due to NaN), which would prevent deriving
/// `PartialEq` on the enum. Using `f64` storage for F32 constants avoids this
/// issue while preserving the value -- the actual narrowing to f32 happens
/// during LLVM lowering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConstValue {
    Bool(bool),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    /// Stored as f64 internally to avoid f32 comparison issues. See module docs.
    F32(f64),
    F64(f64),
    Unit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_all_lm_type_variants() {
        let module = ModuleId(0);

        let types = vec![
            LmType::Scalar(ScalarType::Bool),
            LmType::Scalar(ScalarType::I32),
            LmType::Scalar(ScalarType::F64),
            LmType::Array {
                element: TypeId(1),
                length: 10,
            },
            LmType::Struct(StructDef {
                name: "Point".into(),
                type_id: TypeId(100),
                fields: IndexMap::from([("x".into(), TypeId(5)), ("y".into(), TypeId(5))]),
                module,
                visibility: Visibility::Public,
            }),
            LmType::Enum(EnumDef {
                name: "Option".into(),
                type_id: TypeId(101),
                variants: IndexMap::from([
                    (
                        "None".into(),
                        EnumVariant {
                            index: 0,
                            payload: None,
                        },
                    ),
                    (
                        "Some".into(),
                        EnumVariant {
                            index: 1,
                            payload: Some(TypeId(1)),
                        },
                    ),
                ]),
                module,
                visibility: Visibility::Public,
            }),
            LmType::Pointer {
                pointee: TypeId(1),
                mutable: false,
            },
            LmType::Function {
                params: vec![TypeId(1), TypeId(2)],
                return_type: TypeId(3),
            },
            LmType::Unit,
            LmType::Never,
        ];

        // All 8 variant kinds (Scalar appears multiple times but is one variant kind).
        // Just verify they all construct without panicking.
        assert_eq!(types.len(), 10);
    }

    #[test]
    fn serde_roundtrip_scalar() {
        let ty = LmType::Scalar(ScalarType::I64);
        let json = serde_json::to_string(&ty).unwrap();
        let back: LmType = serde_json::from_str(&json).unwrap();

        // Verify the round-trip produces the same JSON
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_struct_def() {
        let ty = LmType::Struct(StructDef {
            name: "Pair".into(),
            type_id: TypeId(50),
            fields: IndexMap::from([("first".into(), TypeId(1)), ("second".into(), TypeId(2))]),
            module: ModuleId(0),
            visibility: Visibility::Private,
        });

        let json = serde_json::to_string(&ty).unwrap();
        let back: LmType = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_enum_def() {
        let ty = LmType::Enum(EnumDef {
            name: "Result".into(),
            type_id: TypeId(60),
            variants: IndexMap::from([
                (
                    "Ok".into(),
                    EnumVariant {
                        index: 0,
                        payload: Some(TypeId(1)),
                    },
                ),
                (
                    "Err".into(),
                    EnumVariant {
                        index: 1,
                        payload: Some(TypeId(2)),
                    },
                ),
            ]),
            module: ModuleId(0),
            visibility: Visibility::Public,
        });

        let json = serde_json::to_string(&ty).unwrap();
        let back: LmType = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_complex_types() {
        // Function type
        let func = LmType::Function {
            params: vec![TypeId(0), TypeId(1)],
            return_type: TypeId(7),
        };
        let json = serde_json::to_string(&func).unwrap();
        let back: LmType = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&back).unwrap());

        // Pointer type
        let ptr = LmType::Pointer {
            pointee: TypeId(3),
            mutable: true,
        };
        let json = serde_json::to_string(&ptr).unwrap();
        let back: LmType = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&back).unwrap());

        // Array type
        let arr = LmType::Array {
            element: TypeId(4),
            length: 256,
        };
        let json = serde_json::to_string(&arr).unwrap();
        let back: LmType = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&back).unwrap());

        // Unit and Never
        let unit = LmType::Unit;
        let json = serde_json::to_string(&unit).unwrap();
        let back: LmType = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&back).unwrap());

        let never = LmType::Never;
        let json = serde_json::to_string(&never).unwrap();
        let back: LmType = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&back).unwrap());
    }

    #[test]
    fn const_value_variants() {
        let vals = vec![
            ConstValue::Bool(true),
            ConstValue::I8(42),
            ConstValue::I16(1000),
            ConstValue::I32(100_000),
            ConstValue::I64(1_000_000_000),
            ConstValue::F32(3.14),
            ConstValue::F64(2.718281828),
            ConstValue::Unit,
        ];

        for val in &vals {
            let json = serde_json::to_string(val).unwrap();
            let back: ConstValue = serde_json::from_str(&json).unwrap();
            let json2 = serde_json::to_string(&back).unwrap();
            assert_eq!(json, json2);
        }
    }

    #[test]
    fn struct_def_preserves_field_order() {
        let mut fields = IndexMap::new();
        fields.insert("z".to_string(), TypeId(1));
        fields.insert("a".to_string(), TypeId(2));
        fields.insert("m".to_string(), TypeId(3));

        let sd = StructDef {
            name: "Ordered".into(),
            type_id: TypeId(200),
            fields,
            module: ModuleId(0),
            visibility: Visibility::Public,
        };

        let keys: Vec<&str> = sd.fields.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, vec!["z", "a", "m"]);
    }

    #[test]
    fn enum_def_preserves_variant_order() {
        let mut variants = IndexMap::new();
        variants.insert(
            "Third".to_string(),
            EnumVariant {
                index: 0,
                payload: None,
            },
        );
        variants.insert(
            "First".to_string(),
            EnumVariant {
                index: 1,
                payload: None,
            },
        );
        variants.insert(
            "Second".to_string(),
            EnumVariant {
                index: 2,
                payload: Some(TypeId(1)),
            },
        );

        let ed = EnumDef {
            name: "Mixed".into(),
            type_id: TypeId(201),
            variants,
            module: ModuleId(0),
            visibility: Visibility::Private,
        };

        let keys: Vec<&str> = ed.variants.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, vec!["Third", "First", "Second"]);
    }
}

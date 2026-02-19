//! Mapping from lmlang types to LLVM IR types via inkwell.
//!
//! The [`lm_type_to_llvm`] function converts a [`TypeId`] into an inkwell
//! [`BasicTypeEnum`] by looking up the type in the [`TypeRegistry`] and
//! recursively building LLVM types for compound types (arrays, structs,
//! enums, pointers, functions).

use inkwell::context::Context;
use inkwell::types::{BasicType, BasicTypeEnum};
use inkwell::AddressSpace;

use lmlang_core::type_id::{TypeId, TypeRegistry};
use lmlang_core::types::LmType;

use crate::error::CodegenError;

/// Convert an lmlang [`TypeId`] to an LLVM [`BasicTypeEnum`].
///
/// Handles all type variants:
/// - Scalars (Bool, I8, I16, I32, I64, F32, F64) map directly to LLVM primitives.
/// - Unit maps to an empty struct `{}` (zero-size type).
/// - Arrays recursively map element types and create fixed-size LLVM arrays.
/// - Structs map all field types and create LLVM struct types.
/// - Enums become tagged unions `{ i32 discriminant, [max_payload_bytes x i8] }`.
/// - Pointers map to opaque LLVM pointers.
/// - Functions map to function pointer types.
/// - Never is an error (should never appear in codegen).
pub fn lm_type_to_llvm<'ctx>(
    context: &'ctx Context,
    type_id: TypeId,
    registry: &TypeRegistry,
) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    // Handle built-in scalar types by constant ID
    match type_id {
        TypeId::BOOL => return Ok(context.bool_type().into()),
        TypeId::I8 => return Ok(context.i8_type().into()),
        TypeId::I16 => return Ok(context.i16_type().into()),
        TypeId::I32 => return Ok(context.i32_type().into()),
        TypeId::I64 => return Ok(context.i64_type().into()),
        TypeId::F32 => return Ok(context.f32_type().into()),
        TypeId::F64 => return Ok(context.f64_type().into()),
        TypeId::UNIT => return Ok(context.struct_type(&[], false).into()),
        TypeId::NEVER => {
            return Err(CodegenError::TypeMapping(
                "Never type should not appear in codegen".to_string(),
            ))
        }
        _ => {}
    }

    // Look up compound types in the registry
    let lm_type = registry.get(type_id).ok_or_else(|| {
        CodegenError::TypeMapping(format!("type {} not found in registry", type_id))
    })?;

    match lm_type {
        LmType::Scalar(scalar) => {
            // Scalars beyond the built-in range (shouldn't happen, but handle gracefully)
            match scalar {
                lmlang_core::types::ScalarType::Bool => Ok(context.bool_type().into()),
                lmlang_core::types::ScalarType::I8 => Ok(context.i8_type().into()),
                lmlang_core::types::ScalarType::I16 => Ok(context.i16_type().into()),
                lmlang_core::types::ScalarType::I32 => Ok(context.i32_type().into()),
                lmlang_core::types::ScalarType::I64 => Ok(context.i64_type().into()),
                lmlang_core::types::ScalarType::F32 => Ok(context.f32_type().into()),
                lmlang_core::types::ScalarType::F64 => Ok(context.f64_type().into()),
            }
        }
        LmType::Array { element, length } => {
            let elem_ty = lm_type_to_llvm(context, *element, registry)?;
            Ok(elem_ty.array_type(*length).into())
        }
        LmType::Struct(def) => {
            let fields: Vec<BasicTypeEnum<'ctx>> = def
                .fields
                .values()
                .map(|tid| lm_type_to_llvm(context, *tid, registry))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(context.struct_type(&fields, false).into())
        }
        LmType::Enum(def) => {
            // Tagged union: { i32 discriminant, [max_payload_bytes x i8] }
            let max_payload_size = compute_max_payload_size(context, def, registry)?;
            let discriminant_ty = context.i32_type();
            if max_payload_size > 0 {
                let payload_ty = context.i8_type().array_type(max_payload_size);
                Ok(context
                    .struct_type(&[discriminant_ty.into(), payload_ty.into()], false)
                    .into())
            } else {
                // All unit variants -- just the discriminant
                Ok(context.struct_type(&[discriminant_ty.into()], false).into())
            }
        }
        LmType::Pointer { .. } => Ok(context.ptr_type(AddressSpace::default()).into()),
        LmType::Function {
            params,
            return_type,
        } => {
            // Function types are represented as function pointers
            let param_types: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = params
                .iter()
                .map(|tid| lm_type_to_llvm(context, *tid, registry).map(|t| t.into()))
                .collect::<Result<Vec<_>, _>>()?;
            let ret_ty = lm_type_to_llvm(context, *return_type, registry)?;
            let _fn_type = ret_ty.fn_type(&param_types, false);
            // Return as a pointer (function pointer)
            Ok(context.ptr_type(AddressSpace::default()).into())
        }
        LmType::Unit => Ok(context.struct_type(&[], false).into()),
        LmType::Never => Err(CodegenError::TypeMapping(
            "Never type should not appear in codegen".to_string(),
        )),
    }
}

/// Compute the maximum payload size in bytes across all variants of an enum.
///
/// Used to determine the size of the payload field in the tagged union layout.
fn compute_max_payload_size(
    context: &Context,
    def: &lmlang_core::types::EnumDef,
    registry: &TypeRegistry,
) -> Result<u32, CodegenError> {
    let mut max_size: u32 = 0;

    for variant in def.variants.values() {
        if let Some(payload_tid) = variant.payload {
            let payload_ty = lm_type_to_llvm(context, payload_tid, registry)?;
            let size = type_size_bytes(context, payload_ty);
            if size > max_size {
                max_size = size;
            }
        }
    }

    Ok(max_size)
}

/// Estimate the size in bytes of an LLVM type.
///
/// This is a compile-time estimate used for enum payload sizing.
/// For precise sizes, the LLVM target data layout should be used,
/// but this is sufficient for tagged union layout.
fn type_size_bytes(context: &Context, ty: BasicTypeEnum<'_>) -> u32 {
    match ty {
        BasicTypeEnum::IntType(t) => {
            let bits = t.get_bit_width();
            // Round up to next byte
            bits.div_ceil(8)
        }
        BasicTypeEnum::FloatType(t) => {
            // Check if it's f32 or f64 by comparing with known types
            if t == context.f32_type() {
                4
            } else {
                8 // f64 or larger
            }
        }
        BasicTypeEnum::PointerType(_) => 8, // 64-bit pointers
        BasicTypeEnum::ArrayType(t) => {
            let elem_size = type_size_bytes(context, t.get_element_type());
            elem_size * t.len()
        }
        BasicTypeEnum::StructType(t) => {
            let mut total: u32 = 0;
            for i in 0..t.count_fields() {
                total += type_size_bytes(context, t.get_field_type_at_index(i).unwrap());
            }
            total
        }
        BasicTypeEnum::VectorType(_) => 16, // Conservative estimate
        BasicTypeEnum::ScalableVectorType(_) => 16, // Conservative estimate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use lmlang_core::id::ModuleId;
    use lmlang_core::type_id::TypeRegistry;
    use lmlang_core::types::{EnumDef, EnumVariant, LmType, StructDef, Visibility};

    #[test]
    fn scalar_bool_maps_to_i1() {
        let context = Context::create();
        let registry = TypeRegistry::new();
        let ty = lm_type_to_llvm(&context, TypeId::BOOL, &registry).unwrap();
        assert!(ty.is_int_type());
        assert_eq!(ty.into_int_type().get_bit_width(), 1);
    }

    #[test]
    fn scalar_i8_maps_to_i8() {
        let context = Context::create();
        let registry = TypeRegistry::new();
        let ty = lm_type_to_llvm(&context, TypeId::I8, &registry).unwrap();
        assert!(ty.is_int_type());
        assert_eq!(ty.into_int_type().get_bit_width(), 8);
    }

    #[test]
    fn scalar_i16_maps_to_i16() {
        let context = Context::create();
        let registry = TypeRegistry::new();
        let ty = lm_type_to_llvm(&context, TypeId::I16, &registry).unwrap();
        assert!(ty.is_int_type());
        assert_eq!(ty.into_int_type().get_bit_width(), 16);
    }

    #[test]
    fn scalar_i32_maps_to_i32() {
        let context = Context::create();
        let registry = TypeRegistry::new();
        let ty = lm_type_to_llvm(&context, TypeId::I32, &registry).unwrap();
        assert!(ty.is_int_type());
        assert_eq!(ty.into_int_type().get_bit_width(), 32);
    }

    #[test]
    fn scalar_i64_maps_to_i64() {
        let context = Context::create();
        let registry = TypeRegistry::new();
        let ty = lm_type_to_llvm(&context, TypeId::I64, &registry).unwrap();
        assert!(ty.is_int_type());
        assert_eq!(ty.into_int_type().get_bit_width(), 64);
    }

    #[test]
    fn scalar_f32_maps_to_float() {
        let context = Context::create();
        let registry = TypeRegistry::new();
        let ty = lm_type_to_llvm(&context, TypeId::F32, &registry).unwrap();
        assert!(ty.is_float_type());
        assert_eq!(ty.into_float_type(), context.f32_type());
    }

    #[test]
    fn scalar_f64_maps_to_double() {
        let context = Context::create();
        let registry = TypeRegistry::new();
        let ty = lm_type_to_llvm(&context, TypeId::F64, &registry).unwrap();
        assert!(ty.is_float_type());
        assert_eq!(ty.into_float_type(), context.f64_type());
    }

    #[test]
    fn unit_maps_to_empty_struct() {
        let context = Context::create();
        let registry = TypeRegistry::new();
        let ty = lm_type_to_llvm(&context, TypeId::UNIT, &registry).unwrap();
        assert!(ty.is_struct_type());
        assert_eq!(ty.into_struct_type().count_fields(), 0);
    }

    #[test]
    fn never_type_returns_error() {
        let context = Context::create();
        let registry = TypeRegistry::new();
        let result = lm_type_to_llvm(&context, TypeId::NEVER, &registry);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CodegenError::TypeMapping(_)));
    }

    #[test]
    fn array_of_i32() {
        let context = Context::create();
        let mut registry = TypeRegistry::new();
        let arr_id = registry.register(LmType::Array {
            element: TypeId::I32,
            length: 10,
        });
        let ty = lm_type_to_llvm(&context, arr_id, &registry).unwrap();
        assert!(ty.is_array_type());
        let arr_ty = ty.into_array_type();
        assert_eq!(arr_ty.len(), 10);
        assert_eq!(
            arr_ty.get_element_type().into_int_type().get_bit_width(),
            32
        );
    }

    #[test]
    fn struct_type_two_fields() {
        let context = Context::create();
        let mut registry = TypeRegistry::new();
        let struct_id = registry
            .register_named(
                "Point",
                LmType::Struct(StructDef {
                    name: "Point".into(),
                    type_id: TypeId(0), // placeholder
                    fields: IndexMap::from([("x".into(), TypeId::F64), ("y".into(), TypeId::F64)]),
                    module: ModuleId(0),
                    visibility: Visibility::Public,
                }),
            )
            .unwrap();
        let ty = lm_type_to_llvm(&context, struct_id, &registry).unwrap();
        assert!(ty.is_struct_type());
        let struct_ty = ty.into_struct_type();
        assert_eq!(struct_ty.count_fields(), 2);
        // Both fields should be f64
        assert_eq!(
            struct_ty.get_field_type_at_index(0).unwrap(),
            context.f64_type().into()
        );
        assert_eq!(
            struct_ty.get_field_type_at_index(1).unwrap(),
            context.f64_type().into()
        );
    }

    #[test]
    fn enum_all_unit_variants() {
        let context = Context::create();
        let mut registry = TypeRegistry::new();
        let enum_id = registry
            .register_named(
                "Color",
                LmType::Enum(EnumDef {
                    name: "Color".into(),
                    type_id: TypeId(0),
                    variants: IndexMap::from([
                        (
                            "Red".into(),
                            EnumVariant {
                                index: 0,
                                payload: None,
                            },
                        ),
                        (
                            "Green".into(),
                            EnumVariant {
                                index: 1,
                                payload: None,
                            },
                        ),
                        (
                            "Blue".into(),
                            EnumVariant {
                                index: 2,
                                payload: None,
                            },
                        ),
                    ]),
                    module: ModuleId(0),
                    visibility: Visibility::Public,
                }),
            )
            .unwrap();
        let ty = lm_type_to_llvm(&context, enum_id, &registry).unwrap();
        assert!(ty.is_struct_type());
        let struct_ty = ty.into_struct_type();
        // All unit variants: just discriminant (i32), no payload field
        assert_eq!(struct_ty.count_fields(), 1);
        assert_eq!(
            struct_ty.get_field_type_at_index(0).unwrap(),
            context.i32_type().into()
        );
    }

    #[test]
    fn enum_with_payload_variants() {
        let context = Context::create();
        let mut registry = TypeRegistry::new();
        let enum_id = registry
            .register_named(
                "Option",
                LmType::Enum(EnumDef {
                    name: "Option".into(),
                    type_id: TypeId(0),
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
                                payload: Some(TypeId::I64),
                            },
                        ),
                    ]),
                    module: ModuleId(0),
                    visibility: Visibility::Public,
                }),
            )
            .unwrap();
        let ty = lm_type_to_llvm(&context, enum_id, &registry).unwrap();
        assert!(ty.is_struct_type());
        let struct_ty = ty.into_struct_type();
        // Discriminant + payload: { i32, [8 x i8] }
        assert_eq!(struct_ty.count_fields(), 2);
        assert_eq!(
            struct_ty.get_field_type_at_index(0).unwrap(),
            context.i32_type().into()
        );
        // Payload is [8 x i8] for i64 (8 bytes)
        let payload_ty = struct_ty
            .get_field_type_at_index(1)
            .unwrap()
            .into_array_type();
        assert_eq!(payload_ty.len(), 8);
    }

    #[test]
    fn pointer_type_maps_to_opaque_ptr() {
        let context = Context::create();
        let mut registry = TypeRegistry::new();
        let ptr_id = registry.register(LmType::Pointer {
            pointee: TypeId::I32,
            mutable: true,
        });
        let ty = lm_type_to_llvm(&context, ptr_id, &registry).unwrap();
        assert!(ty.is_pointer_type());
    }

    #[test]
    fn function_type_maps_to_function_pointer() {
        let context = Context::create();
        let mut registry = TypeRegistry::new();
        let fn_id = registry.register(LmType::Function {
            params: vec![TypeId::I32, TypeId::I32],
            return_type: TypeId::I64,
        });
        let ty = lm_type_to_llvm(&context, fn_id, &registry).unwrap();
        // Function types are represented as pointers (function pointers)
        assert!(ty.is_pointer_type());
    }

    #[test]
    fn nested_array_of_structs() {
        let context = Context::create();
        let mut registry = TypeRegistry::new();
        let struct_id = registry
            .register_named(
                "Vec2",
                LmType::Struct(StructDef {
                    name: "Vec2".into(),
                    type_id: TypeId(0),
                    fields: IndexMap::from([("x".into(), TypeId::F32), ("y".into(), TypeId::F32)]),
                    module: ModuleId(0),
                    visibility: Visibility::Public,
                }),
            )
            .unwrap();
        let arr_id = registry.register(LmType::Array {
            element: struct_id,
            length: 4,
        });
        let ty = lm_type_to_llvm(&context, arr_id, &registry).unwrap();
        assert!(ty.is_array_type());
        let arr_ty = ty.into_array_type();
        assert_eq!(arr_ty.len(), 4);
        assert!(arr_ty.get_element_type().is_struct_type());
    }

    #[test]
    fn unknown_type_id_returns_error() {
        let context = Context::create();
        let registry = TypeRegistry::new();
        let result = lm_type_to_llvm(&context, TypeId(999), &registry);
        assert!(result.is_err());
    }
}

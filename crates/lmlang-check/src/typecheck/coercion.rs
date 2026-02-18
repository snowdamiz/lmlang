//! Type coercion and widening rules.
//!
//! Defines which implicit type conversions are allowed in data flow edges.
//! Coercion rules follow a conservative, lossless policy:
//!
//! - Bool -> any integer type (true=1, false=0) [LOCKED decision]
//! - Safe integer widening: i8 -> i16 -> i32 -> i64 (same sign family)
//! - Safe float widening: f32 -> f64
//! - &mut T -> &T (mutable to immutable reference)
//! - NO implicit int -> float or float -> int (requires explicit Cast)
//! - NO narrowing conversions

use lmlang_core::type_id::{TypeId, TypeRegistry};
use lmlang_core::types::LmType;

/// Returns `true` if a value of type `from` can implicitly coerce to type `to`.
///
/// This function checks whether an implicit (lossless) conversion exists between
/// two types. It does NOT modify the value -- the actual conversion happens at
/// runtime or via an inserted Cast node.
pub fn can_coerce(from: TypeId, to: TypeId, registry: &TypeRegistry) -> bool {
    // Exact match is always fine (not technically coercion, but handled here
    // for convenience).
    if from == to {
        return true;
    }

    // Bool -> any integer type
    if from == TypeId::BOOL && is_integer(to) {
        return true;
    }

    // Safe integer widening: i8 -> i16 -> i32 -> i64
    if is_integer(from) && is_integer(to) {
        return integer_rank(from) < integer_rank(to);
    }

    // Safe float widening: f32 -> f64
    if from == TypeId::F32 && to == TypeId::F64 {
        return true;
    }

    // &mut T -> &T (mutable to immutable reference coercion)
    if let (Some(LmType::Pointer { pointee: p1, mutable: true }), Some(LmType::Pointer { pointee: p2, mutable: false })) =
        (registry.get(from), registry.get(to))
    {
        return p1 == p2;
    }

    false
}

/// Returns `true` if the type is a numeric type (integer or float).
pub fn is_numeric(type_id: TypeId) -> bool {
    is_integer(type_id) || is_float(type_id)
}

/// Returns `true` if the type is an integer type (I8, I16, I32, I64).
pub fn is_integer(type_id: TypeId) -> bool {
    matches!(
        type_id,
        TypeId { 0: 1 } | TypeId { 0: 2 } | TypeId { 0: 3 } | TypeId { 0: 4 }
    )
}

/// Returns `true` if the type is a float type (F32, F64).
pub fn is_float(type_id: TypeId) -> bool {
    type_id == TypeId::F32 || type_id == TypeId::F64
}

/// Returns `true` if the type is numeric or Bool (Bool can participate in
/// arithmetic via implicit coercion to integer).
pub fn is_numeric_or_bool(type_id: TypeId) -> bool {
    type_id == TypeId::BOOL || is_numeric(type_id)
}

/// Finds the common (wider) numeric type for two types, if one can widen to
/// the other.
///
/// Returns `None` if:
/// - Either type is non-numeric (and not Bool)
/// - The types are in different numeric families (integer vs float)
/// - Neither type can widen to the other
pub fn common_numeric_type(a: TypeId, b: TypeId, _registry: &TypeRegistry) -> Option<TypeId> {
    // Resolve Bool to I8 for arithmetic purposes BEFORE same-type check,
    // because Bool + Bool should produce I8, not Bool.
    let a_resolved = if a == TypeId::BOOL { TypeId::I8 } else { a };
    let b_resolved = if b == TypeId::BOOL { TypeId::I8 } else { b };

    if a_resolved == b_resolved {
        return Some(a_resolved);
    }

    // Both must be numeric after resolution
    if !is_numeric(a_resolved) || !is_numeric(b_resolved) {
        return None;
    }

    // Cannot mix integer and float families
    if is_integer(a_resolved) != is_integer(b_resolved) {
        return None;
    }

    // Within same family, the wider type wins
    if is_integer(a_resolved) {
        let rank_a = integer_rank(a_resolved);
        let rank_b = integer_rank(b_resolved);
        if rank_a >= rank_b {
            Some(a_resolved)
        } else {
            Some(b_resolved)
        }
    } else {
        // Float family: f32 < f64
        if a_resolved == TypeId::F64 || b_resolved == TypeId::F64 {
            Some(TypeId::F64)
        } else {
            Some(TypeId::F32)
        }
    }
}

/// Returns the widening rank of an integer type.
/// Higher rank means wider type. Used for widening comparisons.
fn integer_rank(type_id: TypeId) -> u8 {
    match type_id {
        TypeId::I8 => 1,
        TypeId::I16 => 2,
        TypeId::I32 => 3,
        TypeId::I64 => 4,
        _ => 0, // Non-integer types have rank 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> TypeRegistry {
        TypeRegistry::new()
    }

    // -----------------------------------------------------------------------
    // is_numeric / is_integer / is_float
    // -----------------------------------------------------------------------

    #[test]
    fn integer_types_are_numeric() {
        assert!(is_numeric(TypeId::I8));
        assert!(is_numeric(TypeId::I16));
        assert!(is_numeric(TypeId::I32));
        assert!(is_numeric(TypeId::I64));
    }

    #[test]
    fn float_types_are_numeric() {
        assert!(is_numeric(TypeId::F32));
        assert!(is_numeric(TypeId::F64));
    }

    #[test]
    fn bool_is_not_numeric() {
        assert!(!is_numeric(TypeId::BOOL));
    }

    #[test]
    fn unit_and_never_are_not_numeric() {
        assert!(!is_numeric(TypeId::UNIT));
        assert!(!is_numeric(TypeId::NEVER));
    }

    #[test]
    fn is_integer_correct() {
        assert!(is_integer(TypeId::I8));
        assert!(is_integer(TypeId::I16));
        assert!(is_integer(TypeId::I32));
        assert!(is_integer(TypeId::I64));
        assert!(!is_integer(TypeId::F32));
        assert!(!is_integer(TypeId::BOOL));
    }

    #[test]
    fn is_float_correct() {
        assert!(is_float(TypeId::F32));
        assert!(is_float(TypeId::F64));
        assert!(!is_float(TypeId::I32));
        assert!(!is_float(TypeId::BOOL));
    }

    // -----------------------------------------------------------------------
    // can_coerce
    // -----------------------------------------------------------------------

    #[test]
    fn exact_match_coerces() {
        let reg = registry();
        assert!(can_coerce(TypeId::I32, TypeId::I32, &reg));
        assert!(can_coerce(TypeId::BOOL, TypeId::BOOL, &reg));
    }

    #[test]
    fn bool_to_integer_coerces() {
        let reg = registry();
        assert!(can_coerce(TypeId::BOOL, TypeId::I8, &reg));
        assert!(can_coerce(TypeId::BOOL, TypeId::I16, &reg));
        assert!(can_coerce(TypeId::BOOL, TypeId::I32, &reg));
        assert!(can_coerce(TypeId::BOOL, TypeId::I64, &reg));
    }

    #[test]
    fn bool_to_float_does_not_coerce() {
        let reg = registry();
        assert!(!can_coerce(TypeId::BOOL, TypeId::F32, &reg));
        assert!(!can_coerce(TypeId::BOOL, TypeId::F64, &reg));
    }

    #[test]
    fn integer_widening_coerces() {
        let reg = registry();
        assert!(can_coerce(TypeId::I8, TypeId::I16, &reg));
        assert!(can_coerce(TypeId::I8, TypeId::I32, &reg));
        assert!(can_coerce(TypeId::I8, TypeId::I64, &reg));
        assert!(can_coerce(TypeId::I16, TypeId::I32, &reg));
        assert!(can_coerce(TypeId::I16, TypeId::I64, &reg));
        assert!(can_coerce(TypeId::I32, TypeId::I64, &reg));
    }

    #[test]
    fn integer_narrowing_does_not_coerce() {
        let reg = registry();
        assert!(!can_coerce(TypeId::I64, TypeId::I32, &reg));
        assert!(!can_coerce(TypeId::I32, TypeId::I16, &reg));
        assert!(!can_coerce(TypeId::I16, TypeId::I8, &reg));
    }

    #[test]
    fn float_widening_coerces() {
        let reg = registry();
        assert!(can_coerce(TypeId::F32, TypeId::F64, &reg));
    }

    #[test]
    fn float_narrowing_does_not_coerce() {
        let reg = registry();
        assert!(!can_coerce(TypeId::F64, TypeId::F32, &reg));
    }

    #[test]
    fn int_to_float_does_not_coerce() {
        let reg = registry();
        assert!(!can_coerce(TypeId::I32, TypeId::F32, &reg));
        assert!(!can_coerce(TypeId::I32, TypeId::F64, &reg));
        assert!(!can_coerce(TypeId::I64, TypeId::F64, &reg));
    }

    #[test]
    fn float_to_int_does_not_coerce() {
        let reg = registry();
        assert!(!can_coerce(TypeId::F32, TypeId::I32, &reg));
        assert!(!can_coerce(TypeId::F64, TypeId::I64, &reg));
    }

    #[test]
    fn mut_ref_to_immut_ref_coerces() {
        let mut reg = registry();
        let ptr_mut = reg.register(LmType::Pointer {
            pointee: TypeId::I32,
            mutable: true,
        });
        let ptr_immut = reg.register(LmType::Pointer {
            pointee: TypeId::I32,
            mutable: false,
        });
        assert!(can_coerce(ptr_mut, ptr_immut, &reg));
    }

    #[test]
    fn immut_ref_to_mut_ref_does_not_coerce() {
        let mut reg = registry();
        let ptr_immut = reg.register(LmType::Pointer {
            pointee: TypeId::I32,
            mutable: false,
        });
        let ptr_mut = reg.register(LmType::Pointer {
            pointee: TypeId::I32,
            mutable: true,
        });
        assert!(!can_coerce(ptr_immut, ptr_mut, &reg));
    }

    #[test]
    fn mut_ref_different_pointee_does_not_coerce() {
        let mut reg = registry();
        let ptr_mut_i32 = reg.register(LmType::Pointer {
            pointee: TypeId::I32,
            mutable: true,
        });
        let ptr_immut_i64 = reg.register(LmType::Pointer {
            pointee: TypeId::I64,
            mutable: false,
        });
        assert!(!can_coerce(ptr_mut_i32, ptr_immut_i64, &reg));
    }

    // -----------------------------------------------------------------------
    // common_numeric_type
    // -----------------------------------------------------------------------

    #[test]
    fn common_type_same_type() {
        let reg = registry();
        assert_eq!(common_numeric_type(TypeId::I32, TypeId::I32, &reg), Some(TypeId::I32));
    }

    #[test]
    fn common_type_integer_widening() {
        let reg = registry();
        assert_eq!(common_numeric_type(TypeId::I8, TypeId::I32, &reg), Some(TypeId::I32));
        assert_eq!(common_numeric_type(TypeId::I32, TypeId::I64, &reg), Some(TypeId::I64));
        assert_eq!(common_numeric_type(TypeId::I16, TypeId::I32, &reg), Some(TypeId::I32));
    }

    #[test]
    fn common_type_float_widening() {
        let reg = registry();
        assert_eq!(common_numeric_type(TypeId::F32, TypeId::F64, &reg), Some(TypeId::F64));
    }

    #[test]
    fn common_type_bool_resolves_to_i8() {
        let reg = registry();
        assert_eq!(common_numeric_type(TypeId::BOOL, TypeId::BOOL, &reg), Some(TypeId::I8));
        assert_eq!(common_numeric_type(TypeId::BOOL, TypeId::I32, &reg), Some(TypeId::I32));
    }

    #[test]
    fn common_type_cross_family_returns_none() {
        let reg = registry();
        assert_eq!(common_numeric_type(TypeId::I32, TypeId::F32, &reg), None);
        assert_eq!(common_numeric_type(TypeId::I64, TypeId::F64, &reg), None);
    }

    #[test]
    fn common_type_non_numeric_returns_none() {
        let reg = registry();
        assert_eq!(common_numeric_type(TypeId::UNIT, TypeId::I32, &reg), None);
        assert_eq!(common_numeric_type(TypeId::NEVER, TypeId::F64, &reg), None);
    }
}

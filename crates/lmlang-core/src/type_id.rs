//! TypeId and TypeRegistry for nominal typing.
//!
//! Every type in lmlang has a unique [`TypeId`] providing O(1) identity
//! comparison. The [`TypeRegistry`] manages type registration and lookup,
//! pre-registering the 7 scalar types plus Unit and Never on construction.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::types::{LmType, ScalarType};

/// Unique identifier for a type in the type registry.
///
/// Provides O(1) nominal identity comparison for all types.
/// The inner value is an index into the [`TypeRegistry`]'s type vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeId(pub u32);

impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TypeId({})", self.0)
    }
}

/// Registry of all types in a program, providing nominal identity via [`TypeId`].
///
/// On construction, the registry pre-registers the 9 built-in types:
/// - `TypeId(0)` = Bool
/// - `TypeId(1)` = I8
/// - `TypeId(2)` = I16
/// - `TypeId(3)` = I32
/// - `TypeId(4)` = I64
/// - `TypeId(5)` = F32
/// - `TypeId(6)` = F64
/// - `TypeId(7)` = Unit
/// - `TypeId(8)` = Never
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeRegistry {
    /// Types indexed by TypeId.0
    types: Vec<LmType>,
    /// Named type lookup (for structs and enums)
    names: HashMap<String, TypeId>,
    /// Next available ID
    next_id: u32,
}

/// Pre-registered TypeId constants for built-in types.
impl TypeId {
    pub const BOOL: TypeId = TypeId(0);
    pub const I8: TypeId = TypeId(1);
    pub const I16: TypeId = TypeId(2);
    pub const I32: TypeId = TypeId(3);
    pub const I64: TypeId = TypeId(4);
    pub const F32: TypeId = TypeId(5);
    pub const F64: TypeId = TypeId(6);
    pub const UNIT: TypeId = TypeId(7);
    pub const NEVER: TypeId = TypeId(8);
}

impl TypeRegistry {
    /// Number of built-in types pre-registered on construction.
    const BUILTIN_COUNT: u32 = 9;

    /// Creates a new type registry with built-in scalar types, Unit, and Never
    /// pre-registered.
    ///
    /// Built-in type IDs:
    /// - `TypeId(0)` = Bool
    /// - `TypeId(1)` = I8
    /// - `TypeId(2)` = I16
    /// - `TypeId(3)` = I32
    /// - `TypeId(4)` = I64
    /// - `TypeId(5)` = F32
    /// - `TypeId(6)` = F64
    /// - `TypeId(7)` = Unit
    /// - `TypeId(8)` = Never
    pub fn new() -> Self {
        let types = vec![
            LmType::Scalar(ScalarType::Bool),
            LmType::Scalar(ScalarType::I8),
            LmType::Scalar(ScalarType::I16),
            LmType::Scalar(ScalarType::I32),
            LmType::Scalar(ScalarType::I64),
            LmType::Scalar(ScalarType::F32),
            LmType::Scalar(ScalarType::F64),
            LmType::Unit,
            LmType::Never,
        ];

        TypeRegistry {
            types,
            names: HashMap::new(),
            next_id: Self::BUILTIN_COUNT,
        }
    }

    /// Registers a type and returns its new [`TypeId`].
    ///
    /// The type is added without a name. Use [`register_named`](Self::register_named)
    /// for named types (structs, enums).
    pub fn register(&mut self, ty: LmType) -> TypeId {
        let id = TypeId(self.next_id);
        self.types.push(ty);
        self.next_id += 1;
        id
    }

    /// Registers a named type, returning its [`TypeId`].
    ///
    /// Returns [`CoreError::DuplicateTypeName`] if a type with the same name
    /// already exists.
    pub fn register_named(&mut self, name: &str, ty: LmType) -> Result<TypeId, CoreError> {
        if self.names.contains_key(name) {
            return Err(CoreError::DuplicateTypeName {
                name: name.to_string(),
            });
        }
        let id = self.register(ty);
        self.names.insert(name.to_string(), id);
        Ok(id)
    }

    /// Looks up a type by its [`TypeId`].
    pub fn get(&self, id: TypeId) -> Option<&LmType> {
        self.types.get(id.0 as usize)
    }

    /// Looks up a named type's [`TypeId`] by name.
    pub fn get_by_name(&self, name: &str) -> Option<TypeId> {
        self.names.get(name).copied()
    }

    /// Returns the pre-registered [`TypeId`] for a scalar type.
    pub fn scalar_type_id(&self, scalar: ScalarType) -> TypeId {
        match scalar {
            ScalarType::Bool => TypeId::BOOL,
            ScalarType::I8 => TypeId::I8,
            ScalarType::I16 => TypeId::I16,
            ScalarType::I32 => TypeId::I32,
            ScalarType::I64 => TypeId::I64,
            ScalarType::F32 => TypeId::F32,
            ScalarType::F64 => TypeId::F64,
        }
    }

    /// Returns the pre-registered [`TypeId`] for the Unit type.
    pub fn unit_type_id(&self) -> TypeId {
        TypeId::UNIT
    }

    /// Returns the pre-registered [`TypeId`] for the Never type.
    pub fn never_type_id(&self) -> TypeId {
        TypeId::NEVER
    }
}

impl Default for TypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::ModuleId;
    use crate::types::{EnumDef, EnumVariant, StructDef, Visibility};
    use indexmap::IndexMap;

    #[test]
    fn new_registry_has_9_builtin_types() {
        let reg = TypeRegistry::new();
        // 7 scalars + Unit + Never = 9
        assert_eq!(reg.types.len(), 9);
        assert_eq!(reg.next_id, 9);
    }

    #[test]
    fn builtin_scalar_type_ids() {
        let reg = TypeRegistry::new();

        // Verify each built-in scalar maps to the correct TypeId
        assert_eq!(reg.scalar_type_id(ScalarType::Bool), TypeId(0));
        assert_eq!(reg.scalar_type_id(ScalarType::I8), TypeId(1));
        assert_eq!(reg.scalar_type_id(ScalarType::I16), TypeId(2));
        assert_eq!(reg.scalar_type_id(ScalarType::I32), TypeId(3));
        assert_eq!(reg.scalar_type_id(ScalarType::I64), TypeId(4));
        assert_eq!(reg.scalar_type_id(ScalarType::F32), TypeId(5));
        assert_eq!(reg.scalar_type_id(ScalarType::F64), TypeId(6));
    }

    #[test]
    fn builtin_unit_and_never() {
        let reg = TypeRegistry::new();

        assert_eq!(reg.unit_type_id(), TypeId(7));
        assert_eq!(reg.never_type_id(), TypeId(8));

        // Verify they resolve to the correct types
        assert!(matches!(reg.get(TypeId(7)), Some(LmType::Unit)));
        assert!(matches!(reg.get(TypeId(8)), Some(LmType::Never)));
    }

    #[test]
    fn register_returns_unique_ids() {
        let mut reg = TypeRegistry::new();

        let id1 = reg.register(LmType::Array {
            element: TypeId(3),
            length: 5,
        });
        let id2 = reg.register(LmType::Pointer {
            pointee: TypeId(1),
            mutable: false,
        });

        assert_ne!(id1, id2);
        assert_eq!(id1, TypeId(9));  // First after builtins
        assert_eq!(id2, TypeId(10));
    }

    #[test]
    fn register_named_unique_ids_and_lookup() {
        let mut reg = TypeRegistry::new();

        let point_id = reg
            .register_named(
                "Point",
                LmType::Struct(StructDef {
                    name: "Point".into(),
                    type_id: TypeId(0), // placeholder, will be overwritten
                    fields: IndexMap::from([
                        ("x".into(), TypeId(6)), // F64
                        ("y".into(), TypeId(6)),
                    ]),
                    module: ModuleId(0),
                    visibility: Visibility::Public,
                }),
            )
            .unwrap();

        let color_id = reg
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
                    ]),
                    module: ModuleId(0),
                    visibility: Visibility::Public,
                }),
            )
            .unwrap();

        assert_ne!(point_id, color_id);

        // Lookup by name works
        assert_eq!(reg.get_by_name("Point"), Some(point_id));
        assert_eq!(reg.get_by_name("Color"), Some(color_id));
        assert_eq!(reg.get_by_name("Nonexistent"), None);
    }

    #[test]
    fn duplicate_name_returns_error() {
        let mut reg = TypeRegistry::new();

        reg.register_named("Foo", LmType::Unit).unwrap();

        let result = reg.register_named("Foo", LmType::Unit);
        assert!(result.is_err());

        match result {
            Err(CoreError::DuplicateTypeName { name }) => {
                assert_eq!(name, "Foo");
            }
            _ => panic!("expected DuplicateTypeName error"),
        }
    }

    #[test]
    fn get_and_get_by_name_roundtrip() {
        let mut reg = TypeRegistry::new();

        let id = reg.register_named("MyStruct", LmType::Unit).unwrap();

        // get_by_name returns the ID
        let looked_up_id = reg.get_by_name("MyStruct").unwrap();
        assert_eq!(id, looked_up_id);

        // get with that ID returns the type
        let ty = reg.get(looked_up_id).unwrap();
        assert!(matches!(ty, LmType::Unit));
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let reg = TypeRegistry::new();
        assert!(reg.get(TypeId(999)).is_none());
    }

    #[test]
    fn builtin_types_are_correct() {
        let reg = TypeRegistry::new();

        // Verify each built-in type
        assert!(matches!(
            reg.get(TypeId(0)),
            Some(LmType::Scalar(ScalarType::Bool))
        ));
        assert!(matches!(
            reg.get(TypeId(1)),
            Some(LmType::Scalar(ScalarType::I8))
        ));
        assert!(matches!(
            reg.get(TypeId(2)),
            Some(LmType::Scalar(ScalarType::I16))
        ));
        assert!(matches!(
            reg.get(TypeId(3)),
            Some(LmType::Scalar(ScalarType::I32))
        ));
        assert!(matches!(
            reg.get(TypeId(4)),
            Some(LmType::Scalar(ScalarType::I64))
        ));
        assert!(matches!(
            reg.get(TypeId(5)),
            Some(LmType::Scalar(ScalarType::F32))
        ));
        assert!(matches!(
            reg.get(TypeId(6)),
            Some(LmType::Scalar(ScalarType::F64))
        ));
        assert!(matches!(reg.get(TypeId(7)), Some(LmType::Unit)));
        assert!(matches!(reg.get(TypeId(8)), Some(LmType::Never)));
    }

    #[test]
    fn type_id_display() {
        assert_eq!(format!("{}", TypeId(42)), "TypeId(42)");
    }

    #[test]
    fn serde_roundtrip() {
        let mut reg = TypeRegistry::new();
        reg.register_named("Test", LmType::Unit).unwrap();

        let json = serde_json::to_string(&reg).unwrap();
        let back: TypeRegistry = serde_json::from_str(&json).unwrap();

        // Verify state is preserved
        assert_eq!(back.types.len(), reg.types.len());
        assert_eq!(back.next_id, reg.next_id);
        assert_eq!(back.get_by_name("Test"), Some(TypeId(9)));
    }
}

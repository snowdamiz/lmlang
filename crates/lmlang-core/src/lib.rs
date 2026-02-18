pub mod types;
pub mod type_id;
pub mod id;
pub mod error;

// Re-export commonly used types
pub use types::{LmType, ScalarType, ConstValue, Visibility, StructDef, EnumDef, EnumVariant};
pub use type_id::{TypeId, TypeRegistry};
pub use id::{NodeId, EdgeId, FunctionId, ModuleId};
pub use error::CoreError;

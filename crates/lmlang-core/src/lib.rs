pub mod types;
pub mod type_id;
pub mod id;
pub mod error;
pub mod ops;
pub mod edge;
pub mod node;
pub mod function;
pub mod module;

// Re-export commonly used types
pub use types::{LmType, ScalarType, ConstValue, Visibility, StructDef, EnumDef, EnumVariant};
pub use type_id::{TypeId, TypeRegistry};
pub use id::{NodeId, EdgeId, FunctionId, ModuleId};
pub use error::CoreError;
pub use ops::{ComputeOp, StructuredOp, ComputeNodeOp, ArithOp, CmpOp, LogicOp, ShiftOp, UnaryArithOp};
pub use edge::{FlowEdge, SemanticEdge};
pub use node::{ComputeNode, SemanticNode};
pub use function::{FunctionDef, Capture, CaptureMode};
pub use module::{ModuleDef, ModuleTree};

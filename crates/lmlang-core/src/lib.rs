pub mod edge;
pub mod error;
pub mod function;
pub mod graph;
pub mod id;
pub mod module;
pub mod node;
pub mod ops;
pub mod type_id;
pub mod types;

// Re-export commonly used types
pub use edge::{FlowEdge, SemanticEdge};
pub use error::CoreError;
pub use function::{Capture, CaptureMode, FunctionDef};
pub use graph::{
    ComputeEvent, ConflictPriorityClass, ProgramGraph, PropagationEvent, PropagationEventKind,
    PropagationFlushReport, PropagationLayer, SemanticEvent,
};
pub use id::{EdgeId, FunctionId, ModuleId, NodeId};
pub use module::{ModuleDef, ModuleTree};
pub use node::{
    ComputeNode, DocNode, EmbeddingPayload, FunctionSignature, FunctionSummary, ModuleNode,
    OwnershipMetadata, ProvenanceMetadata, SemanticMetadata, SemanticNode, SemanticSummaryPayload,
    SpecNode, TestNode, TypeDefNode,
};
pub use ops::{
    ArithOp, CmpOp, ComputeNodeOp, ComputeOp, LogicOp, ShiftOp, StructuredOp, UnaryArithOp,
};
pub use type_id::{TypeId, TypeRegistry};
pub use types::{ConstValue, EnumDef, EnumVariant, LmType, ScalarType, StructDef, Visibility};

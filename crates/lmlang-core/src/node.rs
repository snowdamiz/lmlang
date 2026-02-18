//! Node wrappers for both graph layers.
//!
//! The computational graph uses [`ComputeNode`] to wrap operations with function
//! ownership metadata (flat graph with logical function boundaries).
//!
//! The semantic graph uses [`SemanticNode`] to represent module, function, and
//! type definition nodes with structural metadata.

use serde::{Deserialize, Serialize};

use crate::id::{FunctionId, ModuleId};
use crate::ops::{ComputeNodeOp, ComputeOp, StructuredOp};
use crate::type_id::TypeId;
use crate::types::Visibility;

// ---------------------------------------------------------------------------
// Computational graph nodes
// ---------------------------------------------------------------------------

/// A node in the computational graph, wrapping an operation with ownership
/// metadata.
///
/// All compute nodes live in a single flat `StableGraph`. Function boundaries
/// are represented via the `owner` field -- a function's nodes are those with
/// `owner == function_id`. This preserves cross-function edges (direct calls)
/// within one connected graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeNode {
    /// The operation this node performs.
    pub op: ComputeNodeOp,
    /// Which function owns this node. Used to identify function boundaries
    /// in the flat graph.
    pub owner: FunctionId,
}

impl ComputeNode {
    /// Creates a new compute node with the given op and owner.
    pub fn new(op: ComputeNodeOp, owner: FunctionId) -> Self {
        ComputeNode { op, owner }
    }

    /// Creates a compute node wrapping a core (Tier 1) operation.
    pub fn core(op: ComputeOp, owner: FunctionId) -> Self {
        ComputeNode {
            op: ComputeNodeOp::Core(op),
            owner,
        }
    }

    /// Creates a compute node wrapping a structured (Tier 2) operation.
    pub fn structured(op: StructuredOp, owner: FunctionId) -> Self {
        ComputeNode {
            op: ComputeNodeOp::Structured(op),
            owner,
        }
    }

    /// Returns the tier of this node's operation: 1 for core, 2 for structured.
    pub fn tier(&self) -> u8 {
        self.op.tier()
    }

    /// Returns `true` if this node is a control flow operation.
    pub fn is_control_flow(&self) -> bool {
        self.op.is_control_flow()
    }

    /// Returns `true` if this node is a basic block terminator.
    pub fn is_terminator(&self) -> bool {
        self.op.is_terminator()
    }

    /// Returns `true` if this node is an I/O operation.
    pub fn is_io(&self) -> bool {
        self.op.is_io()
    }
}

// ---------------------------------------------------------------------------
// Semantic graph nodes
// ---------------------------------------------------------------------------

/// A node in the semantic graph, representing a module, function, or type
/// definition.
///
/// The semantic graph is a lightweight skeleton in Phase 1: structural
/// containment, function names and signatures, type definitions. No embeddings,
/// summaries, or rich relationships -- those come in later phases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SemanticNode {
    /// A module in the module tree.
    Module(ModuleDef),
    /// A function with its name, signature, and module membership.
    /// Note: This is a summary, not the full `FunctionDef` (which includes
    /// entry nodes, captures, etc. and lives in a separate lookup table).
    Function(FunctionSummary),
    /// A type definition node.
    TypeDef(TypeDefNode),
}

// ---------------------------------------------------------------------------
// Supporting types for SemanticNode
// ---------------------------------------------------------------------------

/// Temporary forward definition of ModuleDef.
///
/// Contains the minimum fields needed for the semantic graph skeleton.
// TODO(plan-03): Move ModuleDef to module.rs when it exists. This stub
// provides the basics so the code compiles and the semantic graph can
// represent module nodes. Plan 03 will add the full definition with
// child tracking and module-level metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleDef {
    /// Module name.
    pub name: String,
    /// Parent module, if any. `None` for the root module.
    pub parent: Option<ModuleId>,
    /// Visibility of this module.
    pub visibility: Visibility,
}

/// Lightweight function summary for the semantic graph.
///
/// Contains just the identity and signature -- enough for structural queries
/// ("what functions exist in this module?", "what's the signature of function X?").
/// The full `FunctionDef` (with entry node, captures, etc.) lives in a
/// separate lookup table, not in the graph node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionSummary {
    /// Function name.
    pub name: String,
    /// Unique function identifier.
    pub function_id: FunctionId,
    /// Module this function belongs to.
    pub module: ModuleId,
    /// Visibility across module boundaries.
    pub visibility: Visibility,
    /// Function type signature (parameters and return type).
    pub signature: FunctionSignature,
}

/// Function type signature: parameter names + types, and return type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionSignature {
    /// Named parameters with their types.
    pub params: Vec<(String, TypeId)>,
    /// Return type of the function.
    pub return_type: TypeId,
}

/// A type definition node for the semantic graph.
///
/// Represents a named type (struct or enum) defined in a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDefNode {
    /// Type name.
    pub name: String,
    /// The TypeId of this type definition.
    pub type_id: TypeId,
    /// Module this type is defined in.
    pub module: ModuleId,
    /// Visibility across module boundaries.
    pub visibility: Visibility,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{ArithOp, ComputeOp, StructuredOp};

    #[test]
    fn compute_node_new() {
        let node = ComputeNode::new(
            ComputeNodeOp::Core(ComputeOp::Alloc),
            FunctionId(1),
        );
        assert_eq!(node.owner, FunctionId(1));
        assert_eq!(node.tier(), 1);
    }

    #[test]
    fn compute_node_core_constructor() {
        let node = ComputeNode::core(
            ComputeOp::BinaryArith { op: ArithOp::Add },
            FunctionId(5),
        );
        assert_eq!(node.owner, FunctionId(5));
        assert_eq!(node.tier(), 1);
        assert!(!node.is_control_flow());
    }

    #[test]
    fn compute_node_structured_constructor() {
        let node = ComputeNode::structured(
            StructuredOp::ArrayGet,
            FunctionId(3),
        );
        assert_eq!(node.owner, FunctionId(3));
        assert_eq!(node.tier(), 2);
    }

    #[test]
    fn compute_node_tier_core_is_1() {
        let node = ComputeNode::core(ComputeOp::Return, FunctionId(0));
        assert_eq!(node.tier(), 1);
    }

    #[test]
    fn compute_node_tier_structured_is_2() {
        let node = ComputeNode::structured(
            StructuredOp::Cast { target_type: TypeId::I64 },
            FunctionId(0),
        );
        assert_eq!(node.tier(), 2);
    }

    #[test]
    fn compute_node_delegates_is_control_flow() {
        let cf_node = ComputeNode::core(ComputeOp::IfElse, FunctionId(0));
        assert!(cf_node.is_control_flow());

        let non_cf = ComputeNode::core(ComputeOp::Alloc, FunctionId(0));
        assert!(!non_cf.is_control_flow());

        let struct_node = ComputeNode::structured(StructuredOp::ArrayGet, FunctionId(0));
        assert!(!struct_node.is_control_flow());
    }

    #[test]
    fn compute_node_delegates_is_terminator() {
        let term = ComputeNode::core(ComputeOp::Return, FunctionId(0));
        assert!(term.is_terminator());

        let non_term = ComputeNode::core(ComputeOp::Print, FunctionId(0));
        assert!(!non_term.is_terminator());
    }

    #[test]
    fn compute_node_delegates_is_io() {
        let io = ComputeNode::core(ComputeOp::FileOpen, FunctionId(0));
        assert!(io.is_io());

        let non_io = ComputeNode::core(ComputeOp::Return, FunctionId(0));
        assert!(!non_io.is_io());
    }

    #[test]
    fn serde_roundtrip_compute_node() {
        let node = ComputeNode::core(
            ComputeOp::BinaryArith { op: ArithOp::Mul },
            FunctionId(42),
        );
        let json = serde_json::to_string(&node).unwrap();
        let back: ComputeNode = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_semantic_node_module() {
        let node = SemanticNode::Module(ModuleDef {
            name: "math".into(),
            parent: Some(ModuleId(0)),
            visibility: Visibility::Public,
        });
        let json = serde_json::to_string(&node).unwrap();
        let back: SemanticNode = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_semantic_node_function() {
        let node = SemanticNode::Function(FunctionSummary {
            name: "add".into(),
            function_id: FunctionId(1),
            module: ModuleId(0),
            visibility: Visibility::Public,
            signature: FunctionSignature {
                params: vec![
                    ("a".into(), TypeId::I32),
                    ("b".into(), TypeId::I32),
                ],
                return_type: TypeId::I32,
            },
        });
        let json = serde_json::to_string(&node).unwrap();
        let back: SemanticNode = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_semantic_node_typedef() {
        let node = SemanticNode::TypeDef(TypeDefNode {
            name: "Point".into(),
            type_id: TypeId(100),
            module: ModuleId(0),
            visibility: Visibility::Private,
        });
        let json = serde_json::to_string(&node).unwrap();
        let back: SemanticNode = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn module_def_root_has_no_parent() {
        let root = ModuleDef {
            name: "root".into(),
            parent: None,
            visibility: Visibility::Public,
        };
        assert!(root.parent.is_none());
    }
}

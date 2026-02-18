//! Edge types for both graph layers.
//!
//! The computational graph uses [`FlowEdge`] to represent data flow (SSA-style
//! typed value passing) and control flow (execution ordering, branch selection).
//! The semantic graph uses [`SemanticEdge`] for structural relationships between
//! modules, functions, and types.

use serde::{Deserialize, Serialize};

use crate::type_id::TypeId;

// ---------------------------------------------------------------------------
// Computational graph edges
// ---------------------------------------------------------------------------

/// Edge types in the computational graph.
///
/// Data flow forms a DAG (directed acyclic graph). Control flow may contain
/// cycles (loops). These are intentionally separate from each other -- LLVM IR
/// distinguishes SSA values from basic block terminators, and keeping them
/// separate allows independent traversal of either flow kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FlowEdge {
    /// SSA-style data dependency. Source node produces a value consumed by target
    /// node. Each node produces at most one value (port 0). Multi-output nodes
    /// (rare) use higher ports.
    Data {
        /// Which output port of the source node (most nodes have port 0 only).
        source_port: u16,
        /// Which input port of the target node.
        target_port: u16,
        /// The type of the value flowing through this edge.
        value_type: TypeId,
    },

    /// Control dependency. Target executes after source. `branch_index` is
    /// `Some(0)` for then-branch, `Some(1)` for else-branch, `None` for
    /// unconditional/sequential ordering.
    Control {
        /// For branches: which branch arm (0 = then, 1 = else, etc.).
        /// `None` for unconditional control flow (sequential, jump).
        branch_index: Option<u16>,
    },
}

impl FlowEdge {
    /// Returns `true` if this is a data flow edge.
    pub fn is_data(&self) -> bool {
        matches!(self, FlowEdge::Data { .. })
    }

    /// Returns `true` if this is a control flow edge.
    pub fn is_control(&self) -> bool {
        matches!(self, FlowEdge::Control { .. })
    }

    /// Returns the value type carried by this edge, if it is a data edge.
    /// Returns `None` for control edges.
    pub fn value_type(&self) -> Option<TypeId> {
        match self {
            FlowEdge::Data { value_type, .. } => Some(*value_type),
            FlowEdge::Control { .. } => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Semantic graph edges
// ---------------------------------------------------------------------------

/// Edge types in the semantic graph.
///
/// These represent structural and reference relationships between modules,
/// functions, and type definitions. The semantic graph is a lightweight
/// skeleton in Phase 1 -- only containment, call, and type-usage relationships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticEdge {
    /// Module contains a child (module, function, or type definition).
    Contains,
    /// Function calls another function.
    Calls,
    /// Function or type references another type.
    UsesType,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_edge_is_data() {
        let data = FlowEdge::Data {
            source_port: 0,
            target_port: 0,
            value_type: TypeId::I32,
        };
        assert!(data.is_data());
        assert!(!data.is_control());
    }

    #[test]
    fn flow_edge_is_control() {
        let ctrl = FlowEdge::Control {
            branch_index: Some(0),
        };
        assert!(ctrl.is_control());
        assert!(!ctrl.is_data());
    }

    #[test]
    fn flow_edge_is_control_unconditional() {
        let ctrl = FlowEdge::Control {
            branch_index: None,
        };
        assert!(ctrl.is_control());
        assert!(!ctrl.is_data());
    }

    #[test]
    fn value_type_returns_some_for_data() {
        let data = FlowEdge::Data {
            source_port: 0,
            target_port: 1,
            value_type: TypeId::F64,
        };
        assert_eq!(data.value_type(), Some(TypeId::F64));
    }

    #[test]
    fn value_type_returns_none_for_control() {
        let ctrl = FlowEdge::Control {
            branch_index: Some(1),
        };
        assert_eq!(ctrl.value_type(), None);
    }

    #[test]
    fn serde_roundtrip_data_edge() {
        let edge = FlowEdge::Data {
            source_port: 0,
            target_port: 2,
            value_type: TypeId::BOOL,
        };
        let json = serde_json::to_string(&edge).unwrap();
        let back: FlowEdge = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_control_edge() {
        let edge = FlowEdge::Control {
            branch_index: Some(0),
        };
        let json = serde_json::to_string(&edge).unwrap();
        let back: FlowEdge = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_control_edge_none() {
        let edge = FlowEdge::Control {
            branch_index: None,
        };
        let json = serde_json::to_string(&edge).unwrap();
        let back: FlowEdge = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn semantic_edge_equality() {
        assert_eq!(SemanticEdge::Contains, SemanticEdge::Contains);
        assert_eq!(SemanticEdge::Calls, SemanticEdge::Calls);
        assert_eq!(SemanticEdge::UsesType, SemanticEdge::UsesType);
        assert_ne!(SemanticEdge::Contains, SemanticEdge::Calls);
    }

    #[test]
    fn serde_roundtrip_semantic_edge() {
        for edge in &[
            SemanticEdge::Contains,
            SemanticEdge::Calls,
            SemanticEdge::UsesType,
        ] {
            let json = serde_json::to_string(edge).unwrap();
            let back: SemanticEdge = serde_json::from_str(&json).unwrap();
            assert_eq!(*edge, back);
        }
    }
}

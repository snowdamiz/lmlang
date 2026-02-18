//! Stable ID newtypes for graph entities.
//!
//! All IDs are distinct newtype wrappers over `u32`, providing type safety
//! so that a `NodeId` cannot be accidentally used where an `EdgeId` is expected.

use std::fmt;

use petgraph::graph::NodeIndex;
use serde::{Deserialize, Serialize};

/// Stable node identifier. Maps to a petgraph `NodeIndex<u32>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

/// Stable edge identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EdgeId(pub u32);

/// Function identity within the program graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionId(pub u32);

/// Module identity within the program graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleId(pub u32);

// Display implementations -- just print the inner value.

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for EdgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for FunctionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// Bridge between NodeId and petgraph's NodeIndex<u32>.

impl From<NodeIndex<u32>> for NodeId {
    fn from(idx: NodeIndex<u32>) -> Self {
        NodeId(idx.index() as u32)
    }
}

impl From<NodeId> for NodeIndex<u32> {
    fn from(id: NodeId) -> Self {
        NodeIndex::new(id.0 as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_to_node_index_roundtrip() {
        let idx = NodeIndex::<u32>::new(42);
        let node_id = NodeId::from(idx);
        assert_eq!(node_id.0, 42);

        let back: NodeIndex<u32> = node_id.into();
        assert_eq!(back.index(), 42);
    }

    #[test]
    fn node_id_display() {
        assert_eq!(format!("{}", NodeId(7)), "7");
    }

    #[test]
    fn edge_id_display() {
        assert_eq!(format!("{}", EdgeId(99)), "99");
    }

    #[test]
    fn function_id_display() {
        assert_eq!(format!("{}", FunctionId(3)), "3");
    }

    #[test]
    fn module_id_display() {
        assert_eq!(format!("{}", ModuleId(0)), "0");
    }

    #[test]
    fn id_types_are_distinct() {
        // Ensure that different ID types cannot be confused at the type level.
        // This is a compile-time guarantee; we just verify the values are independent.
        let node = NodeId(1);
        let edge = EdgeId(1);
        let func = FunctionId(1);
        let module = ModuleId(1);

        // All have the same inner value but are different types.
        assert_eq!(node.0, edge.0);
        assert_eq!(func.0, module.0);
    }

    #[test]
    fn serde_roundtrip() {
        let node = NodeId(42);
        let json = serde_json::to_string(&node).unwrap();
        let back: NodeId = serde_json::from_str(&json).unwrap();
        assert_eq!(node, back);

        let func = FunctionId(7);
        let json = serde_json::to_string(&func).unwrap();
        let back: FunctionId = serde_json::from_str(&json).unwrap();
        assert_eq!(func, back);
    }
}

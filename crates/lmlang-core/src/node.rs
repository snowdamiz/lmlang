//! Node wrappers for both graph layers.
//!
//! The computational graph uses [`ComputeNode`] to wrap operations with function
//! ownership metadata (flat graph with logical function boundaries).
//!
//! The semantic graph uses [`SemanticNode`] to represent richer entities
//! (module/function/type/spec/test/doc) with ownership, provenance, summary,
//! and embedding metadata.

use serde::{Deserialize, Serialize};

use crate::id::{FunctionId, ModuleId};
use crate::module::ModuleDef;
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
/// Phase 8 introduces richer semantic entities and payloads for retrieval and
/// dual-layer synchronization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SemanticNode {
    /// A module in the module tree.
    Module(ModuleNode),
    /// A function with its name, signature, and module membership.
    /// Note: This is a summary, not the full `FunctionDef` (which includes
    /// entry nodes, captures, etc. and lives in a separate lookup table).
    Function(FunctionSummary),
    /// A type definition node.
    TypeDef(TypeDefNode),
    /// A specification artifact (requirements, invariants, behavior contracts).
    Spec(SpecNode),
    /// A test artifact connected to semantic entities.
    Test(TestNode),
    /// A documentation artifact.
    Doc(DocNode),
}

// ---------------------------------------------------------------------------
// Supporting types for SemanticNode
// ---------------------------------------------------------------------------

/// Ownership metadata for semantic entities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct OwnershipMetadata {
    /// Owning module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<ModuleId>,
    /// Owning function, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionId>,
    /// Optional owning domain (e.g. "runtime", "tests", "docs").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

/// Provenance metadata for semantic entities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenanceMetadata {
    /// Source system that generated this semantic artifact.
    pub source: String,
    /// Monotonic semantic version for the artifact.
    pub version: u64,
    /// Creation timestamp (epoch milliseconds).
    pub created_at_ms: u64,
    /// Last update timestamp (epoch milliseconds).
    pub updated_at_ms: u64,
}

impl Default for ProvenanceMetadata {
    fn default() -> Self {
        Self {
            source: "lmlang".to_string(),
            version: 1,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }
}

/// Embeddings for semantic retrieval.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EmbeddingPayload {
    /// Embedding for this semantic node itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_embedding: Option<Vec<f32>>,
    /// Embedding for the enclosing semantic subgraph summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subgraph_summary_embedding: Option<Vec<f32>>,
}

impl EmbeddingPayload {
    /// Returns the node-embedding dimension when present.
    pub fn node_dim(&self) -> Option<usize> {
        self.node_embedding.as_ref().map(Vec::len)
    }

    /// Returns the subgraph-summary embedding dimension when present.
    pub fn summary_dim(&self) -> Option<usize> {
        self.subgraph_summary_embedding.as_ref().map(Vec::len)
    }
}

/// Deterministic summary payload for semantic retrieval/synchronization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SemanticSummaryPayload {
    /// Short deterministic title.
    pub title: String,
    /// Deterministic summary text.
    pub body: String,
    /// Deterministic checksum for stable identity/equality checks.
    pub checksum: String,
    /// Approximate token count.
    pub token_count: u32,
}

impl SemanticSummaryPayload {
    /// Constructs a deterministic summary payload from structured inputs.
    pub fn deterministic(kind: &str, identifier: &str, body: &str) -> Self {
        let normalized = format!("{}|{}|{}", kind, identifier, body.trim());
        Self {
            title: format!("{}:{}", kind, identifier),
            body: body.trim().to_string(),
            checksum: stable_checksum(&normalized),
            token_count: body.split_whitespace().count() as u32,
        }
    }
}

/// Shared semantic metadata bundled with every semantic entity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SemanticMetadata {
    /// Ownership metadata.
    #[serde(default)]
    pub ownership: OwnershipMetadata,
    /// Provenance metadata.
    #[serde(default)]
    pub provenance: ProvenanceMetadata,
    /// Deterministic summary payload.
    #[serde(default)]
    pub summary: SemanticSummaryPayload,
    /// Retrieval embeddings.
    #[serde(default)]
    pub embeddings: EmbeddingPayload,
    /// Optional complexity score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<u32>,
}

impl SemanticMetadata {
    /// Builds metadata with module ownership and deterministic summary.
    pub fn with_module(kind: &str, module: ModuleId, identifier: &str, body: &str) -> Self {
        Self {
            ownership: OwnershipMetadata {
                module: Some(module),
                function: None,
                domain: None,
            },
            provenance: ProvenanceMetadata::default(),
            summary: SemanticSummaryPayload::deterministic(kind, identifier, body),
            embeddings: EmbeddingPayload::default(),
            complexity: None,
        }
    }

    /// Builds metadata with module+function ownership and deterministic summary.
    pub fn with_function(
        kind: &str,
        module: ModuleId,
        function: FunctionId,
        identifier: &str,
        body: &str,
    ) -> Self {
        Self {
            ownership: OwnershipMetadata {
                module: Some(module),
                function: Some(function),
                domain: None,
            },
            provenance: ProvenanceMetadata::default(),
            summary: SemanticSummaryPayload::deterministic(kind, identifier, body),
            embeddings: EmbeddingPayload::default(),
            complexity: None,
        }
    }
}

/// Module semantic node payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleNode {
    /// Module definition.
    pub module: ModuleDef,
    /// Rich metadata (ownership/provenance/summary/embeddings).
    #[serde(default)]
    pub metadata: SemanticMetadata,
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
    /// Rich metadata (ownership/provenance/summary/embeddings).
    #[serde(default)]
    pub metadata: SemanticMetadata,
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
    /// Rich metadata (ownership/provenance/summary/embeddings).
    #[serde(default)]
    pub metadata: SemanticMetadata,
}

/// Spec semantic node payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecNode {
    /// Stable spec identifier.
    pub spec_id: String,
    /// Human-readable title.
    pub title: String,
    /// Rich metadata (ownership/provenance/summary/embeddings).
    #[serde(default)]
    pub metadata: SemanticMetadata,
}

/// Test semantic node payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TestNode {
    /// Stable test identifier.
    pub test_id: String,
    /// Human-readable title.
    pub title: String,
    /// Optional target function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_function: Option<FunctionId>,
    /// Rich metadata (ownership/provenance/summary/embeddings).
    #[serde(default)]
    pub metadata: SemanticMetadata,
}

/// Documentation semantic node payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocNode {
    /// Stable document identifier.
    pub doc_id: String,
    /// Human-readable title.
    pub title: String,
    /// Rich metadata (ownership/provenance/summary/embeddings).
    #[serde(default)]
    pub metadata: SemanticMetadata,
}

impl SemanticNode {
    /// Returns the semantic entity kind.
    pub fn kind(&self) -> &'static str {
        match self {
            SemanticNode::Module(_) => "module",
            SemanticNode::Function(_) => "function",
            SemanticNode::TypeDef(_) => "type",
            SemanticNode::Spec(_) => "spec",
            SemanticNode::Test(_) => "test",
            SemanticNode::Doc(_) => "doc",
        }
    }

    /// Returns a human-oriented label for the entity.
    pub fn label(&self) -> String {
        match self {
            SemanticNode::Module(n) => n.module.name.clone(),
            SemanticNode::Function(n) => n.name.clone(),
            SemanticNode::TypeDef(n) => n.name.clone(),
            SemanticNode::Spec(n) => n.title.clone(),
            SemanticNode::Test(n) => n.title.clone(),
            SemanticNode::Doc(n) => n.title.clone(),
        }
    }

    /// Returns read-only metadata.
    pub fn metadata(&self) -> &SemanticMetadata {
        match self {
            SemanticNode::Module(n) => &n.metadata,
            SemanticNode::Function(n) => &n.metadata,
            SemanticNode::TypeDef(n) => &n.metadata,
            SemanticNode::Spec(n) => &n.metadata,
            SemanticNode::Test(n) => &n.metadata,
            SemanticNode::Doc(n) => &n.metadata,
        }
    }

    /// Returns mutable metadata.
    pub fn metadata_mut(&mut self) -> &mut SemanticMetadata {
        match self {
            SemanticNode::Module(n) => &mut n.metadata,
            SemanticNode::Function(n) => &mut n.metadata,
            SemanticNode::TypeDef(n) => &mut n.metadata,
            SemanticNode::Spec(n) => &mut n.metadata,
            SemanticNode::Test(n) => &mut n.metadata,
            SemanticNode::Doc(n) => &mut n.metadata,
        }
    }

    /// Returns module ownership, if known.
    pub fn module_id(&self) -> Option<ModuleId> {
        match self {
            SemanticNode::Module(n) => Some(n.module.id),
            _ => self.metadata().ownership.module,
        }
    }

    /// Returns function ownership, if known.
    pub fn function_id(&self) -> Option<FunctionId> {
        match self {
            SemanticNode::Function(n) => Some(n.function_id),
            _ => self.metadata().ownership.function,
        }
    }
}

fn stable_checksum(input: &str) -> String {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:016x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{ArithOp, ComputeOp, StructuredOp};

    #[test]
    fn compute_node_new() {
        let node = ComputeNode::new(ComputeNodeOp::Core(ComputeOp::Alloc), FunctionId(1));
        assert_eq!(node.owner, FunctionId(1));
        assert_eq!(node.tier(), 1);
    }

    #[test]
    fn compute_node_core_constructor() {
        let node = ComputeNode::core(ComputeOp::BinaryArith { op: ArithOp::Add }, FunctionId(5));
        assert_eq!(node.owner, FunctionId(5));
        assert_eq!(node.tier(), 1);
        assert!(!node.is_control_flow());
    }

    #[test]
    fn compute_node_structured_constructor() {
        let node = ComputeNode::structured(StructuredOp::ArrayGet, FunctionId(3));
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
            StructuredOp::Cast {
                target_type: TypeId::I64,
            },
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
        let node = ComputeNode::core(ComputeOp::BinaryArith { op: ArithOp::Mul }, FunctionId(42));
        let json = serde_json::to_string(&node).unwrap();
        let back: ComputeNode = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn serde_roundtrip_semantic_node_module() {
        let node = SemanticNode::Module(ModuleNode {
            module: ModuleDef {
                id: ModuleId(1),
                name: "math".into(),
                parent: Some(ModuleId(0)),
                visibility: Visibility::Public,
            },
            metadata: SemanticMetadata::with_module(
                "module",
                ModuleId(1),
                "math",
                "math module docs",
            ),
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
                params: vec![("a".into(), TypeId::I32), ("b".into(), TypeId::I32)],
                return_type: TypeId::I32,
            },
            metadata: SemanticMetadata::with_function(
                "function",
                ModuleId(0),
                FunctionId(1),
                "add",
                "add two numbers",
            ),
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
            metadata: SemanticMetadata::with_module("type", ModuleId(0), "Point", "2d point"),
        });
        let json = serde_json::to_string(&node).unwrap();
        let back: SemanticNode = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn module_def_root_has_no_parent() {
        let root = ModuleDef {
            id: ModuleId(0),
            name: "root".into(),
            parent: None,
            visibility: Visibility::Public,
        };
        assert!(root.parent.is_none());
    }

    #[test]
    fn deterministic_summary_payload_is_stable() {
        let a = SemanticSummaryPayload::deterministic("function", "f", "returns one");
        let b = SemanticSummaryPayload::deterministic("function", "f", "returns one");
        assert_eq!(a, b);
    }

    #[test]
    fn semantic_node_exposes_metadata_and_kind() {
        let node = SemanticNode::Spec(SpecNode {
            spec_id: "SPEC-1".into(),
            title: "No panic".into(),
            metadata: SemanticMetadata::with_module(
                "spec",
                ModuleId(2),
                "SPEC-1",
                "all functions avoid panic",
            ),
        });

        assert_eq!(node.kind(), "spec");
        assert_eq!(node.label(), "No panic");
        assert_eq!(node.module_id(), Some(ModuleId(2)));
        assert!(node.metadata().summary.checksum.len() >= 8);
    }
}

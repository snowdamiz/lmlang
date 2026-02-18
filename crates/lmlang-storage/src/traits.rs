//! The [`GraphStore`] trait defining the storage contract for program graphs.
//!
//! Two-layer API design:
//! - **Low-level CRUD** methods form the trait foundation. Each call writes
//!   exactly one row, serving as the incremental save mechanism.
//! - **High-level convenience** methods (`save_program`, `load_program`,
//!   `save_function`) provide bulk operations built on the CRUD primitives.
//!
//! All backends (InMemoryStore, SQLiteStore, etc.) implement this trait,
//! ensuring they are fully swappable without changing core logic.

use lmlang_core::edge::{FlowEdge, SemanticEdge};
use lmlang_core::function::FunctionDef;
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{EdgeId, FunctionId, ModuleId, NodeId};
use lmlang_core::module::ModuleDef;
use lmlang_core::node::{ComputeNode, SemanticNode};
use lmlang_core::type_id::TypeId;
use lmlang_core::types::LmType;

use crate::error::StorageError;
use crate::types::{ProgramId, ProgramSummary};

/// The storage contract for program graphs.
///
/// Implementations provide persistence for the dual-graph program model.
/// The trait is synchronous (not async) for simplicity in the current
/// single-threaded design.
pub trait GraphStore {
    // -------------------------------------------------------------------
    // Program-level operations
    // -------------------------------------------------------------------

    /// Creates a new empty program with the given name.
    ///
    /// Returns the newly allocated [`ProgramId`].
    fn create_program(&mut self, name: &str) -> Result<ProgramId, StorageError>;

    /// Loads a complete program graph from storage.
    ///
    /// Reconstructs a [`ProgramGraph`] from all stored nodes, edges, types,
    /// functions, modules, and semantic data.
    fn load_program(&self, id: ProgramId) -> Result<ProgramGraph, StorageError>;

    /// Deletes a program and all its associated data.
    fn delete_program(&mut self, id: ProgramId) -> Result<(), StorageError>;

    /// Lists all stored programs.
    fn list_programs(&self) -> Result<Vec<ProgramSummary>, StorageError>;

    // -------------------------------------------------------------------
    // High-level convenience methods
    // -------------------------------------------------------------------

    /// Bulk save/overwrite of an entire program graph.
    ///
    /// Used for initial save of a newly created program or full overwrite.
    /// Decomposes the graph and stores all components.
    fn save_program(&mut self, id: ProgramId, graph: &ProgramGraph) -> Result<(), StorageError>;

    /// Saves/overwrites a single function's data from the given ProgramGraph.
    ///
    /// Extracts nodes owned by `func_id`, their edges, and the function
    /// definition, then replaces the stored data for that function.
    fn save_function(
        &mut self,
        id: ProgramId,
        func_id: FunctionId,
        graph: &ProgramGraph,
    ) -> Result<(), StorageError>;

    // -------------------------------------------------------------------
    // Node CRUD (incremental save)
    // -------------------------------------------------------------------

    /// Inserts a compute node into a program.
    fn insert_node(
        &mut self,
        program: ProgramId,
        node_id: NodeId,
        node: &ComputeNode,
    ) -> Result<(), StorageError>;

    /// Retrieves a compute node by ID.
    fn get_node(
        &self,
        program: ProgramId,
        node_id: NodeId,
    ) -> Result<ComputeNode, StorageError>;

    /// Updates an existing compute node.
    fn update_node(
        &mut self,
        program: ProgramId,
        node_id: NodeId,
        node: &ComputeNode,
    ) -> Result<(), StorageError>;

    /// Deletes a compute node.
    fn delete_node(
        &mut self,
        program: ProgramId,
        node_id: NodeId,
    ) -> Result<(), StorageError>;

    // -------------------------------------------------------------------
    // Edge CRUD
    // -------------------------------------------------------------------

    /// Inserts a flow edge between two nodes.
    fn insert_edge(
        &mut self,
        program: ProgramId,
        edge_id: EdgeId,
        source: NodeId,
        target: NodeId,
        edge: &FlowEdge,
    ) -> Result<(), StorageError>;

    /// Retrieves a flow edge by ID, including source and target node IDs.
    fn get_edge(
        &self,
        program: ProgramId,
        edge_id: EdgeId,
    ) -> Result<(NodeId, NodeId, FlowEdge), StorageError>;

    /// Deletes a flow edge.
    fn delete_edge(
        &mut self,
        program: ProgramId,
        edge_id: EdgeId,
    ) -> Result<(), StorageError>;

    // -------------------------------------------------------------------
    // Type CRUD
    // -------------------------------------------------------------------

    /// Inserts a type into the program's type registry.
    fn insert_type(
        &mut self,
        program: ProgramId,
        type_id: TypeId,
        ty: &LmType,
    ) -> Result<(), StorageError>;

    /// Retrieves a type by ID.
    fn get_type(
        &self,
        program: ProgramId,
        type_id: TypeId,
    ) -> Result<LmType, StorageError>;

    // -------------------------------------------------------------------
    // Function CRUD
    // -------------------------------------------------------------------

    /// Inserts a function definition.
    fn insert_function(
        &mut self,
        program: ProgramId,
        func_id: FunctionId,
        func: &FunctionDef,
    ) -> Result<(), StorageError>;

    /// Retrieves a function definition by ID.
    fn get_function(
        &self,
        program: ProgramId,
        func_id: FunctionId,
    ) -> Result<FunctionDef, StorageError>;

    /// Updates an existing function definition.
    fn update_function(
        &mut self,
        program: ProgramId,
        func_id: FunctionId,
        func: &FunctionDef,
    ) -> Result<(), StorageError>;

    // -------------------------------------------------------------------
    // Module CRUD
    // -------------------------------------------------------------------

    /// Inserts a module definition.
    fn insert_module(
        &mut self,
        program: ProgramId,
        module_id: ModuleId,
        module: &ModuleDef,
    ) -> Result<(), StorageError>;

    /// Retrieves a module definition by ID.
    fn get_module(
        &self,
        program: ProgramId,
        module_id: ModuleId,
    ) -> Result<ModuleDef, StorageError>;

    // -------------------------------------------------------------------
    // Semantic CRUD
    // -------------------------------------------------------------------

    /// Inserts a semantic node at the given index.
    fn insert_semantic_node(
        &mut self,
        program: ProgramId,
        index: u32,
        node: &SemanticNode,
    ) -> Result<(), StorageError>;

    /// Retrieves a semantic node by index.
    fn get_semantic_node(
        &self,
        program: ProgramId,
        index: u32,
    ) -> Result<SemanticNode, StorageError>;

    /// Inserts a semantic edge at the given index.
    fn insert_semantic_edge(
        &mut self,
        program: ProgramId,
        index: u32,
        source: u32,
        target: u32,
        edge: &SemanticEdge,
    ) -> Result<(), StorageError>;

    /// Retrieves a semantic edge by index, including source and target indices.
    fn get_semantic_edge(
        &self,
        program: ProgramId,
        index: u32,
    ) -> Result<(u32, u32, SemanticEdge), StorageError>;

    // -------------------------------------------------------------------
    // Query methods
    // -------------------------------------------------------------------

    /// Finds all compute nodes owned by a function.
    fn find_nodes_by_owner(
        &self,
        program: ProgramId,
        owner: FunctionId,
    ) -> Result<Vec<(NodeId, ComputeNode)>, StorageError>;

    /// Finds all edges originating from a node.
    fn find_edges_from(
        &self,
        program: ProgramId,
        node: NodeId,
    ) -> Result<Vec<(EdgeId, NodeId, FlowEdge)>, StorageError>;

    /// Finds all edges targeting a node.
    fn find_edges_to(
        &self,
        program: ProgramId,
        node: NodeId,
    ) -> Result<Vec<(EdgeId, NodeId, FlowEdge)>, StorageError>;

    /// Finds all functions belonging to a module.
    fn find_functions_in_module(
        &self,
        program: ProgramId,
        module: ModuleId,
    ) -> Result<Vec<(FunctionId, FunctionDef)>, StorageError>;

    /// Finds all compute nodes with a given type in their operation.
    fn find_nodes_by_type(
        &self,
        program: ProgramId,
        type_id: TypeId,
    ) -> Result<Vec<(NodeId, ComputeNode)>, StorageError>;

    /// Lists all functions in a program.
    fn list_functions(
        &self,
        program: ProgramId,
    ) -> Result<Vec<(FunctionId, FunctionDef)>, StorageError>;
}

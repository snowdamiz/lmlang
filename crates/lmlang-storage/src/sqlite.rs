//! SQLite implementation of [`GraphStore`].
//!
//! [`SqliteStore`] persists program graphs in a SQLite database with WAL mode,
//! atomic transactions on every write, and automatic schema migrations.
//! Complex Rust types are stored as JSON TEXT columns via serde_json.

use std::collections::HashMap;

use petgraph::graph::NodeIndex;
use rusqlite::{params, Connection, OptionalExtension};

use lmlang_core::edge::{FlowEdge, SemanticEdge};
use lmlang_core::function::{Capture, FunctionDef};
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{EdgeId, FunctionId, ModuleId, NodeId};
use lmlang_core::module::ModuleDef;
use lmlang_core::node::{ComputeNode, SemanticNode};
use lmlang_core::ops::ComputeNodeOp;
use lmlang_core::type_id::TypeId;
use lmlang_core::types::{LmType, Visibility};

use crate::convert::{decompose, recompose, DecomposedProgram};
use crate::error::StorageError;
use crate::traits::GraphStore;
use crate::types::{ProgramId, ProgramSummary};

/// SQLite-backed implementation of [`GraphStore`].
///
/// Every write operation is wrapped in a transaction for atomicity.
/// The database uses WAL mode for performance and foreign keys for integrity.
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Opens (or creates) a SQLite database at `path`.
    pub fn new(path: &str) -> Result<Self, StorageError> {
        let conn = crate::schema::open_database(path)?;
        Ok(SqliteStore { conn })
    }

    /// Opens an in-memory SQLite database (for testing).
    pub fn in_memory() -> Result<Self, StorageError> {
        let conn = crate::schema::open_in_memory()?;
        Ok(SqliteStore { conn })
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Verifies a program exists, returning an error if not.
    fn assert_program_exists(&self, id: ProgramId) -> Result<(), StorageError> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM programs WHERE id = ?1)",
            params![id.0],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StorageError::ProgramNotFound(id.0));
        }
        Ok(())
    }

    /// Serializes a Visibility to TEXT.
    fn visibility_to_str(v: &Visibility) -> &'static str {
        match v {
            Visibility::Public => "Public",
            Visibility::Private => "Private",
        }
    }

    /// Deserializes a Visibility from TEXT.
    fn str_to_visibility(s: &str) -> Visibility {
        match s {
            "Public" => Visibility::Public,
            _ => Visibility::Private,
        }
    }

    /// Serializes a SemanticEdge to TEXT.
    fn semantic_edge_to_str(e: &SemanticEdge) -> &'static str {
        match e {
            SemanticEdge::Contains => "Contains",
            SemanticEdge::Calls => "Calls",
            SemanticEdge::UsesType => "UsesType",
            SemanticEdge::Documents => "Documents",
            SemanticEdge::Validates => "Validates",
            SemanticEdge::Implements => "Implements",
            SemanticEdge::DependsOn => "DependsOn",
            SemanticEdge::Summarizes => "Summarizes",
            SemanticEdge::Derives => "Derives",
        }
    }

    /// Deserializes a SemanticEdge from TEXT.
    fn str_to_semantic_edge(s: &str) -> SemanticEdge {
        match s {
            "Contains" => SemanticEdge::Contains,
            "Calls" => SemanticEdge::Calls,
            "UsesType" => SemanticEdge::UsesType,
            "Documents" => SemanticEdge::Documents,
            "Validates" => SemanticEdge::Validates,
            "Implements" => SemanticEdge::Implements,
            "DependsOn" => SemanticEdge::DependsOn,
            "Summarizes" => SemanticEdge::Summarizes,
            "Derives" => SemanticEdge::Derives,
            _ => SemanticEdge::Contains, // fallback
        }
    }

    /// Saves all decomposed data into the database within a transaction.
    /// Assumes the program row already exists.
    fn save_decomposed(
        &mut self,
        program_id: i64,
        decomposed: &DecomposedProgram,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;

        // Delete existing data for this program (if any), child tables first
        // to respect foreign key ordering. CASCADE handles this but explicit
        // deletes are clearer.
        tx.execute(
            "DELETE FROM semantic_edges WHERE program_id = ?1",
            params![program_id],
        )?;
        tx.execute(
            "DELETE FROM semantic_nodes WHERE program_id = ?1",
            params![program_id],
        )?;
        tx.execute(
            "DELETE FROM flow_edges WHERE program_id = ?1",
            params![program_id],
        )?;
        tx.execute(
            "DELETE FROM compute_nodes WHERE program_id = ?1",
            params![program_id],
        )?;
        tx.execute(
            "DELETE FROM functions WHERE program_id = ?1",
            params![program_id],
        )?;
        tx.execute(
            "DELETE FROM modules WHERE program_id = ?1",
            params![program_id],
        )?;
        tx.execute(
            "DELETE FROM types WHERE program_id = ?1",
            params![program_id],
        )?;

        // Insert types
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO types (program_id, type_id, type_json, name) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (type_id, ty) in &decomposed.types {
                let type_json = serde_json::to_string(ty)?;
                // Look up the name for this type ID, if any
                let name: Option<String> = decomposed
                    .type_names
                    .iter()
                    .find(|(_, &tid)| tid == *type_id)
                    .map(|(name, _)| name.clone());
                stmt.execute(params![program_id, type_id.0, type_json, name])?;
            }
        }

        // Insert modules (root first, then children -- sort by module_id for determinism)
        {
            let mut sorted_modules: Vec<_> = decomposed.modules.iter().collect();
            sorted_modules.sort_by_key(|(id, _)| id.0);

            let mut stmt = tx.prepare_cached(
                "INSERT INTO modules (program_id, module_id, name, parent_id, visibility) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (module_id, module) in &sorted_modules {
                let parent_id: Option<u32> = module.parent.map(|p| p.0);
                stmt.execute(params![
                    program_id,
                    module_id.0,
                    module.name,
                    parent_id,
                    Self::visibility_to_str(&module.visibility),
                ])?;
            }
        }

        // Insert functions
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO functions (program_id, function_id, name, module_id, visibility, params_json, return_type_id, entry_node_id, is_closure, parent_function, captures_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for (func_id, func) in &decomposed.functions {
                let params_json = serde_json::to_string(&func.params)?;
                let entry_node_id: Option<u32> = func.entry_node.map(|n| n.0);
                let parent_fn: Option<u32> = func.parent_function.map(|f| f.0);
                let captures_json = serde_json::to_string(&func.captures)?;
                stmt.execute(params![
                    program_id,
                    func_id.0,
                    func.name,
                    func.module.0,
                    Self::visibility_to_str(&func.visibility),
                    params_json,
                    func.return_type.0,
                    entry_node_id,
                    func.is_closure as i32,
                    parent_fn,
                    captures_json,
                ])?;
            }
        }

        // Insert compute nodes
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO compute_nodes (program_id, node_id, owner_fn_id, op_json) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (node_id, node) in &decomposed.compute_nodes {
                let op_json = serde_json::to_string(&node.op)?;
                stmt.execute(params![program_id, node_id.0, node.owner.0, op_json])?;
            }
        }

        // Insert flow edges
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO flow_edges (program_id, edge_id, source_id, target_id, edge_json) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (edge_idx, source, target, edge) in &decomposed.flow_edges {
                let edge_json = serde_json::to_string(edge)?;
                stmt.execute(params![
                    program_id, *edge_idx, source.0, target.0, edge_json,
                ])?;
            }
        }

        // Insert semantic nodes
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO semantic_nodes (program_id, node_idx, node_json) VALUES (?1, ?2, ?3)",
            )?;
            for (idx, node) in &decomposed.semantic_nodes {
                let node_json = serde_json::to_string(node)?;
                stmt.execute(params![program_id, *idx, node_json])?;
            }
        }

        // Insert semantic edges
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO semantic_edges (program_id, edge_idx, source_idx, target_idx, edge_type) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (idx, source, target, edge) in &decomposed.semantic_edges {
                stmt.execute(params![
                    program_id,
                    *idx,
                    *source,
                    *target,
                    Self::semantic_edge_to_str(edge),
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Loads all decomposed data from the database for a program.
    fn load_decomposed(&self, program_id: i64) -> Result<DecomposedProgram, StorageError> {
        // Load types
        let types: Vec<(TypeId, LmType)> = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT type_id, type_json FROM types WHERE program_id = ?1 ORDER BY type_id",
            )?;
            let rows = stmt.query_map(params![program_id], |row| {
                let type_id: u32 = row.get(0)?;
                let type_json: String = row.get(1)?;
                Ok((type_id, type_json))
            })?;
            let mut result = Vec::new();
            for row in rows {
                let (type_id, type_json) = row?;
                let ty: LmType = serde_json::from_str(&type_json)?;
                result.push((TypeId(type_id), ty));
            }
            result
        };

        // Load type names
        let type_names: HashMap<String, TypeId> = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT name, type_id FROM types WHERE program_id = ?1 AND name IS NOT NULL",
            )?;
            let rows = stmt.query_map(params![program_id], |row| {
                let name: String = row.get(0)?;
                let type_id: u32 = row.get(1)?;
                Ok((name, TypeId(type_id)))
            })?;
            let mut map = HashMap::new();
            for row in rows {
                let (name, type_id) = row?;
                map.insert(name, type_id);
            }
            map
        };

        // Compute type_next_id from loaded types
        let type_next_id = types.iter().map(|(id, _)| id.0).max().map_or(0, |m| m + 1);

        // Load modules
        let modules: Vec<(ModuleId, ModuleDef)> = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT module_id, name, parent_id, visibility FROM modules WHERE program_id = ?1 ORDER BY module_id",
            )?;
            let rows = stmt.query_map(params![program_id], |row| {
                let module_id: u32 = row.get(0)?;
                let name: String = row.get(1)?;
                let parent_id: Option<u32> = row.get(2)?;
                let visibility: String = row.get(3)?;
                Ok((module_id, name, parent_id, visibility))
            })?;
            let mut result = Vec::new();
            for row in rows {
                let (module_id, name, parent_id, visibility) = row?;
                result.push((
                    ModuleId(module_id),
                    ModuleDef {
                        id: ModuleId(module_id),
                        name,
                        parent: parent_id.map(ModuleId),
                        visibility: Self::str_to_visibility(&visibility),
                    },
                ));
            }
            result
        };

        // Load functions
        let functions: Vec<(FunctionId, FunctionDef)> = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT function_id, name, module_id, visibility, params_json, return_type_id, entry_node_id, is_closure, parent_function, captures_json FROM functions WHERE program_id = ?1 ORDER BY function_id",
            )?;
            let rows = stmt.query_map(params![program_id], |row| {
                let function_id: u32 = row.get(0)?;
                let name: String = row.get(1)?;
                let module_id: u32 = row.get(2)?;
                let visibility: String = row.get(3)?;
                let params_json: String = row.get(4)?;
                let return_type_id: u32 = row.get(5)?;
                let entry_node_id: Option<u32> = row.get(6)?;
                let is_closure: i32 = row.get(7)?;
                let parent_function: Option<u32> = row.get(8)?;
                let captures_json: String = row.get(9)?;
                Ok((
                    function_id,
                    name,
                    module_id,
                    visibility,
                    params_json,
                    return_type_id,
                    entry_node_id,
                    is_closure,
                    parent_function,
                    captures_json,
                ))
            })?;
            let mut result = Vec::new();
            for row in rows {
                let (
                    function_id,
                    name,
                    module_id,
                    visibility,
                    params_json,
                    return_type_id,
                    entry_node_id,
                    is_closure,
                    parent_function,
                    captures_json,
                ) = row?;
                let params: Vec<(String, TypeId)> = serde_json::from_str(&params_json)?;
                let captures: Vec<Capture> = serde_json::from_str(&captures_json)?;
                let func_def = FunctionDef {
                    id: FunctionId(function_id),
                    name,
                    module: ModuleId(module_id),
                    visibility: Self::str_to_visibility(&visibility),
                    params,
                    return_type: TypeId(return_type_id),
                    entry_node: entry_node_id.map(NodeId),
                    captures,
                    is_closure: is_closure != 0,
                    parent_function: parent_function.map(FunctionId),
                };
                result.push((FunctionId(function_id), func_def));
            }
            result
        };

        // Compute next_function_id
        let next_function_id = functions
            .iter()
            .map(|(id, _)| id.0)
            .max()
            .map_or(0, |m| m + 1);

        // Load compute nodes
        let compute_nodes: Vec<(NodeId, ComputeNode)> = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT node_id, owner_fn_id, op_json FROM compute_nodes WHERE program_id = ?1 ORDER BY node_id",
            )?;
            let rows = stmt.query_map(params![program_id], |row| {
                let node_id: u32 = row.get(0)?;
                let owner_fn_id: u32 = row.get(1)?;
                let op_json: String = row.get(2)?;
                Ok((node_id, owner_fn_id, op_json))
            })?;
            let mut result = Vec::new();
            for row in rows {
                let (node_id, owner_fn_id, op_json) = row?;
                let op: ComputeNodeOp = serde_json::from_str(&op_json)?;
                result.push((
                    NodeId(node_id),
                    ComputeNode {
                        op,
                        owner: FunctionId(owner_fn_id),
                    },
                ));
            }
            result
        };

        // Load flow edges
        let flow_edges: Vec<(u32, NodeId, NodeId, FlowEdge)> = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT edge_id, source_id, target_id, edge_json FROM flow_edges WHERE program_id = ?1 ORDER BY edge_id",
            )?;
            let rows = stmt.query_map(params![program_id], |row| {
                let edge_id: u32 = row.get(0)?;
                let source_id: u32 = row.get(1)?;
                let target_id: u32 = row.get(2)?;
                let edge_json: String = row.get(3)?;
                Ok((edge_id, source_id, target_id, edge_json))
            })?;
            let mut result = Vec::new();
            for row in rows {
                let (edge_id, source_id, target_id, edge_json) = row?;
                let edge: FlowEdge = serde_json::from_str(&edge_json)?;
                result.push((edge_id, NodeId(source_id), NodeId(target_id), edge));
            }
            result
        };

        // Load semantic nodes
        let semantic_nodes: Vec<(u32, SemanticNode)> = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT node_idx, node_json FROM semantic_nodes WHERE program_id = ?1 ORDER BY node_idx",
            )?;
            let rows = stmt.query_map(params![program_id], |row| {
                let idx: u32 = row.get(0)?;
                let node_json: String = row.get(1)?;
                Ok((idx, node_json))
            })?;
            let mut result = Vec::new();
            for row in rows {
                let (idx, node_json) = row?;
                let node: SemanticNode = serde_json::from_str(&node_json)?;
                result.push((idx, node));
            }
            result
        };

        // Load semantic edges
        let semantic_edges: Vec<(u32, u32, u32, SemanticEdge)> = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT edge_idx, source_idx, target_idx, edge_type FROM semantic_edges WHERE program_id = ?1 ORDER BY edge_idx",
            )?;
            let rows = stmt.query_map(params![program_id], |row| {
                let idx: u32 = row.get(0)?;
                let source: u32 = row.get(1)?;
                let target: u32 = row.get(2)?;
                let edge_type: String = row.get(3)?;
                Ok((idx, source, target, edge_type))
            })?;
            let mut result = Vec::new();
            for row in rows {
                let (idx, source, target, edge_type) = row?;
                result.push((idx, source, target, Self::str_to_semantic_edge(&edge_type)));
            }
            result
        };

        // Rebuild ModuleTree from loaded modules and functions
        let module_tree = self.rebuild_module_tree(program_id, &modules, &functions)?;

        // Rebuild module_semantic_indices and function_semantic_indices
        let mut module_semantic_indices: HashMap<ModuleId, NodeIndex<u32>> = HashMap::new();
        let mut function_semantic_indices: HashMap<FunctionId, NodeIndex<u32>> = HashMap::new();

        for (idx, node) in &semantic_nodes {
            let node_index = NodeIndex::<u32>::new(*idx as usize);
            match node {
                SemanticNode::Module(m) => {
                    module_semantic_indices.insert(m.module.id, node_index);
                }
                SemanticNode::Function(f) => {
                    function_semantic_indices.insert(f.function_id, node_index);
                }
                SemanticNode::TypeDef(_)
                | SemanticNode::Spec(_)
                | SemanticNode::Test(_)
                | SemanticNode::Doc(_) => {
                    // TypeDef nodes don't have a separate index map currently
                }
            }
        }

        Ok(DecomposedProgram {
            compute_nodes,
            flow_edges,
            types,
            type_names,
            type_next_id,
            functions,
            modules,
            module_tree,
            semantic_nodes,
            semantic_edges,
            module_semantic_indices,
            function_semantic_indices,
            next_function_id,
        })
    }

    /// Rebuilds a ModuleTree from loaded module and function data.
    fn rebuild_module_tree(
        &self,
        program_id: i64,
        modules: &[(ModuleId, ModuleDef)],
        functions: &[(FunctionId, FunctionDef)],
    ) -> Result<lmlang_core::module::ModuleTree, StorageError> {
        let mut module_map: HashMap<ModuleId, ModuleDef> = HashMap::new();
        let mut children_map: HashMap<ModuleId, Vec<ModuleId>> = HashMap::new();
        let mut functions_map: HashMap<ModuleId, Vec<FunctionId>> = HashMap::new();
        let mut type_defs_map: HashMap<ModuleId, Vec<TypeId>> = HashMap::new();

        // Find root module (parent is None)
        let root_id = modules
            .iter()
            .find(|(_, m)| m.parent.is_none())
            .map(|(id, _)| *id)
            .ok_or_else(|| StorageError::IntegrityError {
                reason: format!("no root module found for program {}", program_id),
            })?;

        // Build module map and initialize children/functions for each module
        for (id, module) in modules {
            module_map.insert(*id, module.clone());
            children_map.entry(*id).or_default();
            functions_map.entry(*id).or_default();
            type_defs_map.entry(*id).or_default();
        }

        // Build parent-child relationships
        for (id, module) in modules {
            if let Some(parent) = module.parent {
                children_map.entry(parent).or_default().push(*id);
            }
        }

        // Build module-function relationships
        for (func_id, func) in functions {
            functions_map.entry(func.module).or_default().push(*func_id);
        }

        // Load type_defs from database
        {
            let mut stmt = self.conn.prepare_cached(
                "SELECT type_id FROM types WHERE program_id = ?1 AND name IS NOT NULL ORDER BY type_id",
            )?;
            let rows = stmt.query_map(params![program_id], |row| {
                let type_id: u32 = row.get(0)?;
                Ok(type_id)
            })?;
            // For now, all named types belong to the root module (since we don't
            // track module membership of types in the types table yet).
            for row in rows {
                let type_id = row?;
                type_defs_map
                    .entry(root_id)
                    .or_default()
                    .push(TypeId(type_id));
            }
        }

        // Compute next_id from max module_id
        let next_id = modules
            .iter()
            .map(|(id, _)| id.0)
            .max()
            .map_or(1, |m| m + 1);

        Ok(lmlang_core::module::ModuleTree::from_parts(
            module_map,
            children_map,
            functions_map,
            type_defs_map,
            root_id,
            next_id,
        ))
    }
}

impl GraphStore for SqliteStore {
    // -------------------------------------------------------------------
    // Program-level operations
    // -------------------------------------------------------------------

    fn create_program(&mut self, name: &str) -> Result<ProgramId, StorageError> {
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO programs (name) VALUES (?1)", params![name])?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(ProgramId(id))
    }

    fn load_program(&self, id: ProgramId) -> Result<ProgramGraph, StorageError> {
        self.assert_program_exists(id)?;
        let decomposed = self.load_decomposed(id.0)?;
        recompose(decomposed)
    }

    fn delete_program(&mut self, id: ProgramId) -> Result<(), StorageError> {
        self.assert_program_exists(id)?;
        let tx = self.conn.transaction()?;
        // Delete child tables first (respect FK ordering), then program
        tx.execute(
            "DELETE FROM semantic_edges WHERE program_id = ?1",
            params![id.0],
        )?;
        tx.execute(
            "DELETE FROM semantic_nodes WHERE program_id = ?1",
            params![id.0],
        )?;
        tx.execute(
            "DELETE FROM flow_edges WHERE program_id = ?1",
            params![id.0],
        )?;
        tx.execute(
            "DELETE FROM compute_nodes WHERE program_id = ?1",
            params![id.0],
        )?;
        tx.execute("DELETE FROM functions WHERE program_id = ?1", params![id.0])?;
        tx.execute("DELETE FROM modules WHERE program_id = ?1", params![id.0])?;
        tx.execute("DELETE FROM types WHERE program_id = ?1", params![id.0])?;
        tx.execute("DELETE FROM programs WHERE id = ?1", params![id.0])?;
        tx.commit()?;
        Ok(())
    }

    fn list_programs(&self) -> Result<Vec<ProgramSummary>, StorageError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, name FROM programs ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            Ok(ProgramSummary {
                id: ProgramId(id),
                name,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // -------------------------------------------------------------------
    // High-level convenience methods
    // -------------------------------------------------------------------

    fn save_program(&mut self, id: ProgramId, graph: &ProgramGraph) -> Result<(), StorageError> {
        self.assert_program_exists(id)?;
        let decomposed = decompose(graph);
        self.save_decomposed(id.0, &decomposed)?;
        Ok(())
    }

    fn save_function(
        &mut self,
        id: ProgramId,
        func_id: FunctionId,
        graph: &ProgramGraph,
    ) -> Result<(), StorageError> {
        self.assert_program_exists(id)?;
        let tx = self.conn.transaction()?;

        // Delete existing compute_nodes and flow_edges for this function
        // First get the node IDs owned by this function
        let old_node_ids: Vec<u32> = {
            let mut stmt = tx.prepare_cached(
                "SELECT node_id FROM compute_nodes WHERE program_id = ?1 AND owner_fn_id = ?2",
            )?;
            let rows = stmt.query_map(params![id.0, func_id.0], |row| {
                let node_id: u32 = row.get(0)?;
                Ok(node_id)
            })?;
            let mut ids = Vec::new();
            for row in rows {
                ids.push(row?);
            }
            ids
        };

        // Delete edges connected to those nodes
        for node_id in &old_node_ids {
            tx.execute(
                "DELETE FROM flow_edges WHERE program_id = ?1 AND (source_id = ?2 OR target_id = ?2)",
                params![id.0, *node_id],
            )?;
        }

        // Delete the old nodes
        tx.execute(
            "DELETE FROM compute_nodes WHERE program_id = ?1 AND owner_fn_id = ?2",
            params![id.0, func_id.0],
        )?;

        // Delete old function row
        tx.execute(
            "DELETE FROM functions WHERE program_id = ?1 AND function_id = ?2",
            params![id.0, func_id.0],
        )?;

        // Insert fresh nodes from graph
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO compute_nodes (program_id, node_id, owner_fn_id, op_json) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for idx in graph.compute().node_indices() {
                let node = graph.compute().node_weight(idx).unwrap();
                if node.owner == func_id {
                    let op_json = serde_json::to_string(&node.op)?;
                    let node_id = idx.index() as u32;
                    stmt.execute(params![id.0, node_id, node.owner.0, op_json])?;
                }
            }
        }

        // Insert edges connected to this function's nodes
        {
            use petgraph::visit::{EdgeRef, IntoEdgeReferences};
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO flow_edges (program_id, edge_id, source_id, target_id, edge_json) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for edge_ref in graph.compute().edge_references() {
                let src_node = graph.compute().node_weight(edge_ref.source());
                let tgt_node = graph.compute().node_weight(edge_ref.target());
                if src_node.map_or(false, |n| n.owner == func_id)
                    || tgt_node.map_or(false, |n| n.owner == func_id)
                {
                    let edge_json = serde_json::to_string(edge_ref.weight())?;
                    stmt.execute(params![
                        id.0,
                        edge_ref.id().index() as u32,
                        edge_ref.source().index() as u32,
                        edge_ref.target().index() as u32,
                        edge_json,
                    ])?;
                }
            }
        }

        // Insert updated function definition
        if let Some(func_def) = graph.get_function(func_id) {
            let params_json = serde_json::to_string(&func_def.params)?;
            let captures_json = serde_json::to_string(&func_def.captures)?;
            let entry_node_id: Option<u32> = func_def.entry_node.map(|n| n.0);
            let parent_fn: Option<u32> = func_def.parent_function.map(|f| f.0);
            tx.execute(
                "INSERT INTO functions (program_id, function_id, name, module_id, visibility, params_json, return_type_id, entry_node_id, is_closure, parent_function, captures_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    id.0,
                    func_id.0,
                    func_def.name,
                    func_def.module.0,
                    Self::visibility_to_str(&func_def.visibility),
                    params_json,
                    func_def.return_type.0,
                    entry_node_id,
                    func_def.is_closure as i32,
                    parent_fn,
                    captures_json,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    // -------------------------------------------------------------------
    // Node CRUD
    // -------------------------------------------------------------------

    fn insert_node(
        &mut self,
        program: ProgramId,
        node_id: NodeId,
        node: &ComputeNode,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let op_json = serde_json::to_string(&node.op)?;
        tx.execute(
            "INSERT INTO compute_nodes (program_id, node_id, owner_fn_id, op_json) VALUES (?1, ?2, ?3, ?4)",
            params![program.0, node_id.0, node.owner.0, op_json],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn get_node(&self, program: ProgramId, node_id: NodeId) -> Result<ComputeNode, StorageError> {
        let row: Option<(u32, String)> = self
            .conn
            .query_row(
                "SELECT owner_fn_id, op_json FROM compute_nodes WHERE program_id = ?1 AND node_id = ?2",
                params![program.0, node_id.0],
                |row| {
                    let owner: u32 = row.get(0)?;
                    let op_json: String = row.get(1)?;
                    Ok((owner, op_json))
                },
            )
            .optional()?;

        match row {
            Some((owner, op_json)) => {
                let op: ComputeNodeOp = serde_json::from_str(&op_json)?;
                Ok(ComputeNode {
                    op,
                    owner: FunctionId(owner),
                })
            }
            None => Err(StorageError::NodeNotFound {
                program: program.0,
                node: node_id.0,
            }),
        }
    }

    fn update_node(
        &mut self,
        program: ProgramId,
        node_id: NodeId,
        node: &ComputeNode,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let op_json = serde_json::to_string(&node.op)?;
        let rows = tx.execute(
            "UPDATE compute_nodes SET owner_fn_id = ?3, op_json = ?4 WHERE program_id = ?1 AND node_id = ?2",
            params![program.0, node_id.0, node.owner.0, op_json],
        )?;
        tx.commit()?;
        if rows == 0 {
            return Err(StorageError::NodeNotFound {
                program: program.0,
                node: node_id.0,
            });
        }
        Ok(())
    }

    fn delete_node(&mut self, program: ProgramId, node_id: NodeId) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let rows = tx.execute(
            "DELETE FROM compute_nodes WHERE program_id = ?1 AND node_id = ?2",
            params![program.0, node_id.0],
        )?;
        tx.commit()?;
        if rows == 0 {
            return Err(StorageError::NodeNotFound {
                program: program.0,
                node: node_id.0,
            });
        }
        Ok(())
    }

    // -------------------------------------------------------------------
    // Edge CRUD
    // -------------------------------------------------------------------

    fn insert_edge(
        &mut self,
        program: ProgramId,
        edge_id: EdgeId,
        source: NodeId,
        target: NodeId,
        edge: &FlowEdge,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let edge_json = serde_json::to_string(edge)?;
        tx.execute(
            "INSERT INTO flow_edges (program_id, edge_id, source_id, target_id, edge_json) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![program.0, edge_id.0, source.0, target.0, edge_json],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn get_edge(
        &self,
        program: ProgramId,
        edge_id: EdgeId,
    ) -> Result<(NodeId, NodeId, FlowEdge), StorageError> {
        let row: Option<(u32, u32, String)> = self
            .conn
            .query_row(
                "SELECT source_id, target_id, edge_json FROM flow_edges WHERE program_id = ?1 AND edge_id = ?2",
                params![program.0, edge_id.0],
                |row| {
                    let source: u32 = row.get(0)?;
                    let target: u32 = row.get(1)?;
                    let edge_json: String = row.get(2)?;
                    Ok((source, target, edge_json))
                },
            )
            .optional()?;

        match row {
            Some((source, target, edge_json)) => {
                let edge: FlowEdge = serde_json::from_str(&edge_json)?;
                Ok((NodeId(source), NodeId(target), edge))
            }
            None => Err(StorageError::EdgeNotFound {
                program: program.0,
                edge: edge_id.0,
            }),
        }
    }

    fn delete_edge(&mut self, program: ProgramId, edge_id: EdgeId) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let rows = tx.execute(
            "DELETE FROM flow_edges WHERE program_id = ?1 AND edge_id = ?2",
            params![program.0, edge_id.0],
        )?;
        tx.commit()?;
        if rows == 0 {
            return Err(StorageError::EdgeNotFound {
                program: program.0,
                edge: edge_id.0,
            });
        }
        Ok(())
    }

    // -------------------------------------------------------------------
    // Type CRUD
    // -------------------------------------------------------------------

    fn insert_type(
        &mut self,
        program: ProgramId,
        type_id: TypeId,
        ty: &LmType,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let type_json = serde_json::to_string(ty)?;
        tx.execute(
            "INSERT INTO types (program_id, type_id, type_json) VALUES (?1, ?2, ?3)",
            params![program.0, type_id.0, type_json],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn get_type(&self, program: ProgramId, type_id: TypeId) -> Result<LmType, StorageError> {
        let row: Option<String> = self
            .conn
            .query_row(
                "SELECT type_json FROM types WHERE program_id = ?1 AND type_id = ?2",
                params![program.0, type_id.0],
                |row| row.get(0),
            )
            .optional()?;

        match row {
            Some(type_json) => {
                let ty: LmType = serde_json::from_str(&type_json)?;
                Ok(ty)
            }
            None => Err(StorageError::TypeNotFound {
                program: program.0,
                type_id: type_id.0,
            }),
        }
    }

    // -------------------------------------------------------------------
    // Function CRUD
    // -------------------------------------------------------------------

    fn insert_function(
        &mut self,
        program: ProgramId,
        func_id: FunctionId,
        func: &FunctionDef,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let params_json = serde_json::to_string(&func.params)?;
        let captures_json = serde_json::to_string(&func.captures)?;
        let entry_node_id: Option<u32> = func.entry_node.map(|n| n.0);
        let parent_fn: Option<u32> = func.parent_function.map(|f| f.0);
        tx.execute(
            "INSERT INTO functions (program_id, function_id, name, module_id, visibility, params_json, return_type_id, entry_node_id, is_closure, parent_function, captures_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                program.0,
                func_id.0,
                func.name,
                func.module.0,
                Self::visibility_to_str(&func.visibility),
                params_json,
                func.return_type.0,
                entry_node_id,
                func.is_closure as i32,
                parent_fn,
                captures_json,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn get_function(
        &self,
        program: ProgramId,
        func_id: FunctionId,
    ) -> Result<FunctionDef, StorageError> {
        let row = self
            .conn
            .query_row(
                "SELECT name, module_id, visibility, params_json, return_type_id, entry_node_id, is_closure, parent_function, captures_json FROM functions WHERE program_id = ?1 AND function_id = ?2",
                params![program.0, func_id.0],
                |row| {
                    let name: String = row.get(0)?;
                    let module_id: u32 = row.get(1)?;
                    let visibility: String = row.get(2)?;
                    let params_json: String = row.get(3)?;
                    let return_type_id: u32 = row.get(4)?;
                    let entry_node_id: Option<u32> = row.get(5)?;
                    let is_closure: i32 = row.get(6)?;
                    let parent_function: Option<u32> = row.get(7)?;
                    let captures_json: String = row.get(8)?;
                    Ok((name, module_id, visibility, params_json, return_type_id, entry_node_id, is_closure, parent_function, captures_json))
                },
            )
            .optional()?;

        match row {
            Some((
                name,
                module_id,
                visibility,
                params_json,
                return_type_id,
                entry_node_id,
                is_closure,
                parent_function,
                captures_json,
            )) => {
                let params: Vec<(String, TypeId)> = serde_json::from_str(&params_json)?;
                let captures: Vec<Capture> = serde_json::from_str(&captures_json)?;
                Ok(FunctionDef {
                    id: func_id,
                    name,
                    module: ModuleId(module_id),
                    visibility: Self::str_to_visibility(&visibility),
                    params,
                    return_type: TypeId(return_type_id),
                    entry_node: entry_node_id.map(NodeId),
                    captures,
                    is_closure: is_closure != 0,
                    parent_function: parent_function.map(FunctionId),
                })
            }
            None => Err(StorageError::FunctionNotFound {
                program: program.0,
                function: func_id.0,
            }),
        }
    }

    fn update_function(
        &mut self,
        program: ProgramId,
        func_id: FunctionId,
        func: &FunctionDef,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let params_json = serde_json::to_string(&func.params)?;
        let captures_json = serde_json::to_string(&func.captures)?;
        let entry_node_id: Option<u32> = func.entry_node.map(|n| n.0);
        let parent_fn: Option<u32> = func.parent_function.map(|f| f.0);
        let rows = tx.execute(
            "UPDATE functions SET name = ?3, module_id = ?4, visibility = ?5, params_json = ?6, return_type_id = ?7, entry_node_id = ?8, is_closure = ?9, parent_function = ?10, captures_json = ?11 WHERE program_id = ?1 AND function_id = ?2",
            params![
                program.0,
                func_id.0,
                func.name,
                func.module.0,
                Self::visibility_to_str(&func.visibility),
                params_json,
                func.return_type.0,
                entry_node_id,
                func.is_closure as i32,
                parent_fn,
                captures_json,
            ],
        )?;
        tx.commit()?;
        if rows == 0 {
            return Err(StorageError::FunctionNotFound {
                program: program.0,
                function: func_id.0,
            });
        }
        Ok(())
    }

    // -------------------------------------------------------------------
    // Module CRUD
    // -------------------------------------------------------------------

    fn insert_module(
        &mut self,
        program: ProgramId,
        module_id: ModuleId,
        module: &ModuleDef,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let parent_id: Option<u32> = module.parent.map(|p| p.0);
        tx.execute(
            "INSERT INTO modules (program_id, module_id, name, parent_id, visibility) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                program.0,
                module_id.0,
                module.name,
                parent_id,
                Self::visibility_to_str(&module.visibility),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn get_module(
        &self,
        program: ProgramId,
        module_id: ModuleId,
    ) -> Result<ModuleDef, StorageError> {
        let row = self
            .conn
            .query_row(
                "SELECT name, parent_id, visibility FROM modules WHERE program_id = ?1 AND module_id = ?2",
                params![program.0, module_id.0],
                |row| {
                    let name: String = row.get(0)?;
                    let parent_id: Option<u32> = row.get(1)?;
                    let visibility: String = row.get(2)?;
                    Ok((name, parent_id, visibility))
                },
            )
            .optional()?;

        match row {
            Some((name, parent_id, visibility)) => Ok(ModuleDef {
                id: module_id,
                name,
                parent: parent_id.map(ModuleId),
                visibility: Self::str_to_visibility(&visibility),
            }),
            None => Err(StorageError::ModuleNotFound {
                program: program.0,
                module: module_id.0,
            }),
        }
    }

    // -------------------------------------------------------------------
    // Semantic CRUD
    // -------------------------------------------------------------------

    fn insert_semantic_node(
        &mut self,
        program: ProgramId,
        index: u32,
        node: &SemanticNode,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let node_json = serde_json::to_string(node)?;
        tx.execute(
            "INSERT INTO semantic_nodes (program_id, node_idx, node_json) VALUES (?1, ?2, ?3)",
            params![program.0, index, node_json],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn get_semantic_node(
        &self,
        program: ProgramId,
        index: u32,
    ) -> Result<SemanticNode, StorageError> {
        let row: Option<String> = self
            .conn
            .query_row(
                "SELECT node_json FROM semantic_nodes WHERE program_id = ?1 AND node_idx = ?2",
                params![program.0, index],
                |row| row.get(0),
            )
            .optional()?;

        match row {
            Some(node_json) => {
                let node: SemanticNode = serde_json::from_str(&node_json)?;
                Ok(node)
            }
            None => Err(StorageError::IntegrityError {
                reason: format!("semantic node {} not found in program {}", index, program.0),
            }),
        }
    }

    fn insert_semantic_edge(
        &mut self,
        program: ProgramId,
        index: u32,
        source: u32,
        target: u32,
        edge: &SemanticEdge,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO semantic_edges (program_id, edge_idx, source_idx, target_idx, edge_type) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                program.0,
                index,
                source,
                target,
                Self::semantic_edge_to_str(edge),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn get_semantic_edge(
        &self,
        program: ProgramId,
        index: u32,
    ) -> Result<(u32, u32, SemanticEdge), StorageError> {
        let row = self
            .conn
            .query_row(
                "SELECT source_idx, target_idx, edge_type FROM semantic_edges WHERE program_id = ?1 AND edge_idx = ?2",
                params![program.0, index],
                |row| {
                    let source: u32 = row.get(0)?;
                    let target: u32 = row.get(1)?;
                    let edge_type: String = row.get(2)?;
                    Ok((source, target, edge_type))
                },
            )
            .optional()?;

        match row {
            Some((source, target, edge_type)) => {
                Ok((source, target, Self::str_to_semantic_edge(&edge_type)))
            }
            None => Err(StorageError::IntegrityError {
                reason: format!("semantic edge {} not found in program {}", index, program.0),
            }),
        }
    }

    // -------------------------------------------------------------------
    // Query methods
    // -------------------------------------------------------------------

    fn find_nodes_by_owner(
        &self,
        program: ProgramId,
        owner: FunctionId,
    ) -> Result<Vec<(NodeId, ComputeNode)>, StorageError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT node_id, op_json FROM compute_nodes WHERE program_id = ?1 AND owner_fn_id = ?2 ORDER BY node_id",
        )?;
        let rows = stmt.query_map(params![program.0, owner.0], |row| {
            let node_id: u32 = row.get(0)?;
            let op_json: String = row.get(1)?;
            Ok((node_id, op_json))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (node_id, op_json) = row?;
            let op: ComputeNodeOp = serde_json::from_str(&op_json)?;
            result.push((NodeId(node_id), ComputeNode { op, owner }));
        }
        Ok(result)
    }

    fn find_edges_from(
        &self,
        program: ProgramId,
        node: NodeId,
    ) -> Result<Vec<(EdgeId, NodeId, FlowEdge)>, StorageError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT edge_id, target_id, edge_json FROM flow_edges WHERE program_id = ?1 AND source_id = ?2 ORDER BY edge_id",
        )?;
        let rows = stmt.query_map(params![program.0, node.0], |row| {
            let edge_id: u32 = row.get(0)?;
            let target_id: u32 = row.get(1)?;
            let edge_json: String = row.get(2)?;
            Ok((edge_id, target_id, edge_json))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (edge_id, target_id, edge_json) = row?;
            let edge: FlowEdge = serde_json::from_str(&edge_json)?;
            result.push((EdgeId(edge_id), NodeId(target_id), edge));
        }
        Ok(result)
    }

    fn find_edges_to(
        &self,
        program: ProgramId,
        node: NodeId,
    ) -> Result<Vec<(EdgeId, NodeId, FlowEdge)>, StorageError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT edge_id, source_id, edge_json FROM flow_edges WHERE program_id = ?1 AND target_id = ?2 ORDER BY edge_id",
        )?;
        let rows = stmt.query_map(params![program.0, node.0], |row| {
            let edge_id: u32 = row.get(0)?;
            let source_id: u32 = row.get(1)?;
            let edge_json: String = row.get(2)?;
            Ok((edge_id, source_id, edge_json))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (edge_id, source_id, edge_json) = row?;
            let edge: FlowEdge = serde_json::from_str(&edge_json)?;
            result.push((EdgeId(edge_id), NodeId(source_id), edge));
        }
        Ok(result)
    }

    fn find_functions_in_module(
        &self,
        program: ProgramId,
        module: ModuleId,
    ) -> Result<Vec<(FunctionId, FunctionDef)>, StorageError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT function_id, name, visibility, params_json, return_type_id, entry_node_id, is_closure, parent_function, captures_json FROM functions WHERE program_id = ?1 AND module_id = ?2 ORDER BY function_id",
        )?;
        let rows = stmt.query_map(params![program.0, module.0], |row| {
            let function_id: u32 = row.get(0)?;
            let name: String = row.get(1)?;
            let visibility: String = row.get(2)?;
            let params_json: String = row.get(3)?;
            let return_type_id: u32 = row.get(4)?;
            let entry_node_id: Option<u32> = row.get(5)?;
            let is_closure: i32 = row.get(6)?;
            let parent_function: Option<u32> = row.get(7)?;
            let captures_json: String = row.get(8)?;
            Ok((
                function_id,
                name,
                visibility,
                params_json,
                return_type_id,
                entry_node_id,
                is_closure,
                parent_function,
                captures_json,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (
                function_id,
                name,
                visibility,
                params_json,
                return_type_id,
                entry_node_id,
                is_closure,
                parent_function,
                captures_json,
            ) = row?;
            let params: Vec<(String, TypeId)> = serde_json::from_str(&params_json)?;
            let captures: Vec<Capture> = serde_json::from_str(&captures_json)?;
            result.push((
                FunctionId(function_id),
                FunctionDef {
                    id: FunctionId(function_id),
                    name,
                    module,
                    visibility: Self::str_to_visibility(&visibility),
                    params,
                    return_type: TypeId(return_type_id),
                    entry_node: entry_node_id.map(NodeId),
                    captures,
                    is_closure: is_closure != 0,
                    parent_function: parent_function.map(FunctionId),
                },
            ));
        }
        Ok(result)
    }

    fn find_nodes_by_type(
        &self,
        program: ProgramId,
        type_id: TypeId,
    ) -> Result<Vec<(NodeId, ComputeNode)>, StorageError> {
        // Types are inferred from edges, not stored on nodes.
        // Find nodes that are source or target of edges carrying this type.
        let mut stmt = self.conn.prepare_cached(
            "SELECT DISTINCT cn.node_id, cn.owner_fn_id, cn.op_json
             FROM compute_nodes cn
             JOIN flow_edges fe ON cn.program_id = fe.program_id
               AND (cn.node_id = fe.source_id OR cn.node_id = fe.target_id)
             WHERE cn.program_id = ?1
             ORDER BY cn.node_id",
        )?;
        let rows = stmt.query_map(params![program.0], |row| {
            let node_id: u32 = row.get(0)?;
            let owner: u32 = row.get(1)?;
            let op_json: String = row.get(2)?;
            Ok((node_id, owner, op_json))
        })?;

        // Collect all candidate nodes, then filter by edge type
        let mut candidates: Vec<(NodeId, ComputeNode)> = Vec::new();
        for row in rows {
            let (node_id, owner, op_json) = row?;
            let op: ComputeNodeOp = serde_json::from_str(&op_json)?;
            candidates.push((
                NodeId(node_id),
                ComputeNode {
                    op,
                    owner: FunctionId(owner),
                },
            ));
        }

        // Now find nodes whose edges carry the given type
        let mut node_ids_with_type = std::collections::HashSet::new();
        {
            let mut stmt = self.conn.prepare_cached(
                "SELECT source_id, target_id, edge_json FROM flow_edges WHERE program_id = ?1",
            )?;
            let rows = stmt.query_map(params![program.0], |row| {
                let source: u32 = row.get(0)?;
                let target: u32 = row.get(1)?;
                let edge_json: String = row.get(2)?;
                Ok((source, target, edge_json))
            })?;
            for row in rows {
                let (source, target, edge_json) = row?;
                let edge: FlowEdge = serde_json::from_str(&edge_json)?;
                if edge.value_type() == Some(type_id) {
                    node_ids_with_type.insert(source);
                    node_ids_with_type.insert(target);
                }
            }
        }

        Ok(candidates
            .into_iter()
            .filter(|(id, _)| node_ids_with_type.contains(&id.0))
            .collect())
    }

    fn list_functions(
        &self,
        program: ProgramId,
    ) -> Result<Vec<(FunctionId, FunctionDef)>, StorageError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT function_id, name, module_id, visibility, params_json, return_type_id, entry_node_id, is_closure, parent_function, captures_json FROM functions WHERE program_id = ?1 ORDER BY function_id",
        )?;
        let rows = stmt.query_map(params![program.0], |row| {
            let function_id: u32 = row.get(0)?;
            let name: String = row.get(1)?;
            let module_id: u32 = row.get(2)?;
            let visibility: String = row.get(3)?;
            let params_json: String = row.get(4)?;
            let return_type_id: u32 = row.get(5)?;
            let entry_node_id: Option<u32> = row.get(6)?;
            let is_closure: i32 = row.get(7)?;
            let parent_function: Option<u32> = row.get(8)?;
            let captures_json: String = row.get(9)?;
            Ok((
                function_id,
                name,
                module_id,
                visibility,
                params_json,
                return_type_id,
                entry_node_id,
                is_closure,
                parent_function,
                captures_json,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (
                function_id,
                name,
                module_id,
                visibility,
                params_json,
                return_type_id,
                entry_node_id,
                is_closure,
                parent_function,
                captures_json,
            ) = row?;
            let params: Vec<(String, TypeId)> = serde_json::from_str(&params_json)?;
            let captures: Vec<Capture> = serde_json::from_str(&captures_json)?;
            result.push((
                FunctionId(function_id),
                FunctionDef {
                    id: FunctionId(function_id),
                    name,
                    module: ModuleId(module_id),
                    visibility: Self::str_to_visibility(&visibility),
                    params,
                    return_type: TypeId(return_type_id),
                    entry_node: entry_node_id.map(NodeId),
                    captures,
                    is_closure: is_closure != 0,
                    parent_function: parent_function.map(FunctionId),
                },
            ));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::function::{Capture, CaptureMode};
    use lmlang_core::ops::{ArithOp, ComputeOp, UnaryArithOp};
    use lmlang_core::types::Visibility;

    /// Builds the multi-function closure program from Phase 1 integration test.
    /// Same program as InMemoryStore tests: add, make_adder, adder(closure).
    fn build_full_program() -> ProgramGraph {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();
        let i32_id = TypeId::I32;

        // Function "add"
        let add_fn_id = graph
            .add_function(
                "add".into(),
                root,
                vec![("a".into(), i32_id), ("b".into(), i32_id)],
                i32_id,
                Visibility::Public,
            )
            .unwrap();

        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, add_fn_id)
            .unwrap();
        let param_b = graph
            .add_core_op(ComputeOp::Parameter { index: 1 }, add_fn_id)
            .unwrap();
        let sum = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, add_fn_id)
            .unwrap();
        let add_ret = graph.add_core_op(ComputeOp::Return, add_fn_id).unwrap();

        graph.add_data_edge(param_a, sum, 0, 0, i32_id).unwrap();
        graph.add_data_edge(param_b, sum, 0, 1, i32_id).unwrap();
        graph.add_data_edge(sum, add_ret, 0, 0, i32_id).unwrap();

        graph.get_function_mut(add_fn_id).unwrap().entry_node = Some(param_a);

        // Register function type
        let fn_i32_to_i32 = graph.types.register(LmType::Function {
            params: vec![i32_id],
            return_type: i32_id,
        });

        // Function "make_adder"
        let make_adder_id = graph
            .add_function(
                "make_adder".into(),
                root,
                vec![("offset".into(), i32_id)],
                fn_i32_to_i32,
                Visibility::Public,
            )
            .unwrap();

        // Closure "adder"
        let adder_fn_id = graph
            .add_closure(
                "adder".into(),
                root,
                make_adder_id,
                vec![("x".into(), i32_id)],
                i32_id,
                vec![Capture {
                    name: "offset".into(),
                    captured_type: i32_id,
                    mode: CaptureMode::ByValue,
                }],
            )
            .unwrap();

        // Closure body
        let param_x = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, adder_fn_id)
            .unwrap();
        let cap_offset = graph
            .add_core_op(ComputeOp::CaptureAccess { index: 0 }, adder_fn_id)
            .unwrap();
        let call_add = graph
            .add_core_op(ComputeOp::Call { target: add_fn_id }, adder_fn_id)
            .unwrap();
        let adder_ret = graph.add_core_op(ComputeOp::Return, adder_fn_id).unwrap();

        graph
            .add_data_edge(param_x, call_add, 0, 0, i32_id)
            .unwrap();
        graph
            .add_data_edge(cap_offset, call_add, 0, 1, i32_id)
            .unwrap();
        graph
            .add_data_edge(call_add, adder_ret, 0, 0, i32_id)
            .unwrap();

        graph.get_function_mut(adder_fn_id).unwrap().entry_node = Some(param_x);

        // make_adder body
        let ma_param_offset = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, make_adder_id)
            .unwrap();
        let make_closure = graph
            .add_core_op(
                ComputeOp::MakeClosure {
                    function: adder_fn_id,
                },
                make_adder_id,
            )
            .unwrap();
        let ma_ret = graph.add_core_op(ComputeOp::Return, make_adder_id).unwrap();

        graph
            .add_data_edge(ma_param_offset, make_closure, 0, 0, i32_id)
            .unwrap();
        graph
            .add_data_edge(make_closure, ma_ret, 0, 0, fn_i32_to_i32)
            .unwrap();

        graph.get_function_mut(make_adder_id).unwrap().entry_node = Some(ma_param_offset);

        graph
    }

    #[test]
    fn test_create_and_list_programs() {
        let mut store = SqliteStore::in_memory().unwrap();

        let id1 = store.create_program("alpha").unwrap();
        let id2 = store.create_program("beta").unwrap();

        let list = store.list_programs().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "alpha");
        assert_eq!(list[0].id, id1);
        assert_eq!(list[1].name, "beta");
        assert_eq!(list[1].id, id2);

        // Duplicate name should fail (UNIQUE constraint)
        let dup = store.create_program("alpha");
        assert!(dup.is_err());
    }

    #[test]
    fn test_save_load_empty_program() {
        let mut store = SqliteStore::in_memory().unwrap();
        let graph = ProgramGraph::new("main");

        let id = store.create_program("empty").unwrap();
        store.save_program(id, &graph).unwrap();

        let loaded = store.load_program(id).unwrap();

        // Empty program has: 0 compute nodes, 0 edges, 0 functions
        assert_eq!(loaded.node_count(), 0);
        assert_eq!(loaded.edge_count(), 0);
        assert_eq!(loaded.function_count(), 0);
        // But has 1 semantic node (root module) and 0 semantic edges
        assert_eq!(loaded.semantic_node_count(), 1);
        assert_eq!(loaded.semantic_edge_count(), 0);
    }

    #[test]
    fn test_save_load_full_program_roundtrip() {
        let mut store = SqliteStore::in_memory().unwrap();
        let graph = build_full_program();

        let id = store.create_program("full_test").unwrap();
        store.save_program(id, &graph).unwrap();

        let loaded = store.load_program(id).unwrap();

        // Verify counts match exactly
        assert_eq!(loaded.node_count(), 11); // add: 4, adder: 4, make_adder: 3
        assert_eq!(loaded.edge_count(), 8); // add: 3, adder: 3, make_adder: 2
        assert_eq!(loaded.function_count(), 3); // add, make_adder, adder

        // Verify semantic graph
        assert_eq!(loaded.semantic_node_count(), 4); // root + 3 functions
        assert_eq!(loaded.semantic_edge_count(), 3); // Contains edges

        // Verify function "add"
        let add_def = loaded.get_function(FunctionId(0)).unwrap();
        assert_eq!(add_def.name, "add");
        assert_eq!(add_def.params.len(), 2);
        assert_eq!(add_def.return_type, TypeId::I32);
        assert!(add_def.entry_node.is_some());
        assert!(!add_def.is_closure);

        // Verify function "make_adder"
        let make_adder_def = loaded.get_function(FunctionId(1)).unwrap();
        assert_eq!(make_adder_def.name, "make_adder");
        assert_eq!(make_adder_def.params.len(), 1);
        assert!(make_adder_def.entry_node.is_some());

        // Verify closure "adder"
        let adder_def = loaded.get_function(FunctionId(2)).unwrap();
        assert!(adder_def.is_closure);
        assert_eq!(adder_def.captures.len(), 1);
        assert_eq!(adder_def.captures[0].name, "offset");
        assert_eq!(adder_def.captures[0].mode, CaptureMode::ByValue);
        assert_eq!(adder_def.parent_function, Some(FunctionId(1)));

        // Verify function_nodes returns correct count for "add"
        let add_nodes = loaded.function_nodes(FunctionId(0));
        assert_eq!(add_nodes.len(), 4);

        // Verify entry node op
        if let Some(entry_id) = add_def.entry_node {
            let entry_node = loaded.get_compute_node(entry_id).unwrap();
            assert!(matches!(
                entry_node.op,
                lmlang_core::ops::ComputeNodeOp::Core(ComputeOp::Parameter { index: 0 })
            ));
        }
    }

    #[test]
    fn test_delete_program() {
        let mut store = SqliteStore::in_memory().unwrap();
        let graph = build_full_program();

        let id = store.create_program("to_delete").unwrap();
        store.save_program(id, &graph).unwrap();

        // Verify it loads
        assert!(store.load_program(id).is_ok());

        // Delete
        store.delete_program(id).unwrap();

        // Load should now fail
        let result = store.load_program(id);
        assert!(result.is_err());
        match result.unwrap_err() {
            StorageError::ProgramNotFound(pid) => assert_eq!(pid, id.0),
            other => panic!("expected ProgramNotFound, got: {:?}", other),
        }
    }

    #[test]
    fn test_crud_individual_node() {
        let mut store = SqliteStore::in_memory().unwrap();
        let graph = ProgramGraph::new("main");

        let id = store.create_program("crud_node_test").unwrap();
        store.save_program(id, &graph).unwrap();

        // We need a function and module in the DB for FK constraints.
        // The save_program above saved the root module. Add a function.
        let func = FunctionDef::new(
            FunctionId(0),
            "test_fn".into(),
            ModuleId(0),
            vec![],
            TypeId::UNIT,
        );
        store.insert_function(id, FunctionId(0), &func).unwrap();

        // Insert a node
        let node = ComputeNode::core(ComputeOp::Parameter { index: 0 }, FunctionId(0));
        store.insert_node(id, NodeId(100), &node).unwrap();

        // Get node
        let retrieved = store.get_node(id, NodeId(100)).unwrap();
        assert_eq!(retrieved.owner, FunctionId(0));
        assert!(matches!(
            retrieved.op,
            lmlang_core::ops::ComputeNodeOp::Core(ComputeOp::Parameter { index: 0 })
        ));

        // Update node
        let updated_node = ComputeNode::core(ComputeOp::Return, FunctionId(0));
        store.update_node(id, NodeId(100), &updated_node).unwrap();
        let retrieved2 = store.get_node(id, NodeId(100)).unwrap();
        assert!(matches!(
            retrieved2.op,
            lmlang_core::ops::ComputeNodeOp::Core(ComputeOp::Return)
        ));

        // Delete node
        store.delete_node(id, NodeId(100)).unwrap();
        let result = store.get_node(id, NodeId(100));
        assert!(result.is_err());
        match result.unwrap_err() {
            StorageError::NodeNotFound { program, node } => {
                assert_eq!(program, id.0);
                assert_eq!(node, 100);
            }
            other => panic!("expected NodeNotFound, got: {:?}", other),
        }
    }

    #[test]
    fn test_crud_individual_edge() {
        let mut store = SqliteStore::in_memory().unwrap();
        let graph = ProgramGraph::new("main");

        let id = store.create_program("crud_edge_test").unwrap();
        store.save_program(id, &graph).unwrap();

        // Add function and two nodes for FK constraints
        let func = FunctionDef::new(
            FunctionId(0),
            "test_fn".into(),
            ModuleId(0),
            vec![],
            TypeId::UNIT,
        );
        store.insert_function(id, FunctionId(0), &func).unwrap();

        let node_a = ComputeNode::core(ComputeOp::Parameter { index: 0 }, FunctionId(0));
        let node_b = ComputeNode::core(ComputeOp::Return, FunctionId(0));
        store.insert_node(id, NodeId(0), &node_a).unwrap();
        store.insert_node(id, NodeId(1), &node_b).unwrap();

        // Insert edge
        let edge = FlowEdge::Data {
            source_port: 0,
            target_port: 0,
            value_type: TypeId::I32,
        };
        store
            .insert_edge(id, EdgeId(0), NodeId(0), NodeId(1), &edge)
            .unwrap();

        // Get edge
        let (src, tgt, retrieved) = store.get_edge(id, EdgeId(0)).unwrap();
        assert_eq!(src, NodeId(0));
        assert_eq!(tgt, NodeId(1));
        assert!(retrieved.is_data());
        assert_eq!(retrieved.value_type(), Some(TypeId::I32));

        // Delete edge
        store.delete_edge(id, EdgeId(0)).unwrap();
        let result = store.get_edge(id, EdgeId(0));
        assert!(result.is_err());
        match result.unwrap_err() {
            StorageError::EdgeNotFound { program, edge } => {
                assert_eq!(program, id.0);
                assert_eq!(edge, 0);
            }
            other => panic!("expected EdgeNotFound, got: {:?}", other),
        }
    }

    #[test]
    fn test_semantic_edge_roundtrip_extended_variants() {
        let mut store = SqliteStore::in_memory().unwrap();
        let graph = ProgramGraph::new("main");
        let id = store.create_program("semantic_edge_variant_test").unwrap();
        store.save_program(id, &graph).unwrap();

        let extra_node = SemanticNode::Doc(lmlang_core::node::DocNode {
            doc_id: "DOC-1".to_string(),
            title: "Doc".to_string(),
            metadata: lmlang_core::node::SemanticMetadata::default(),
        });
        store.insert_semantic_node(id, 1, &extra_node).unwrap();

        store
            .insert_semantic_edge(id, 0, 0, 1, &SemanticEdge::Documents)
            .unwrap();
        let (_, _, edge) = store.get_semantic_edge(id, 0).unwrap();
        assert_eq!(edge, SemanticEdge::Documents);
    }

    #[test]
    fn test_query_nodes_by_owner() {
        let mut store = SqliteStore::in_memory().unwrap();
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();
        let i32_id = TypeId::I32;

        // Create two functions
        let fn1 = graph
            .add_function("fn1".into(), root, vec![], i32_id, Visibility::Public)
            .unwrap();
        let fn2 = graph
            .add_function("fn2".into(), root, vec![], i32_id, Visibility::Public)
            .unwrap();

        // Add 3 nodes to fn1, 2 to fn2
        graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, fn1)
            .unwrap();
        graph
            .add_core_op(ComputeOp::Parameter { index: 1 }, fn1)
            .unwrap();
        graph.add_core_op(ComputeOp::Return, fn1).unwrap();

        graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, fn2)
            .unwrap();
        graph.add_core_op(ComputeOp::Return, fn2).unwrap();

        let id = store.create_program("query_test").unwrap();
        store.save_program(id, &graph).unwrap();

        // Query nodes by owner
        let fn1_nodes = store.find_nodes_by_owner(id, fn1).unwrap();
        assert_eq!(fn1_nodes.len(), 3);

        let fn2_nodes = store.find_nodes_by_owner(id, fn2).unwrap();
        assert_eq!(fn2_nodes.len(), 2);

        // Nonexistent function returns empty
        let fn99_nodes = store.find_nodes_by_owner(id, FunctionId(99)).unwrap();
        assert_eq!(fn99_nodes.len(), 0);
    }

    #[test]
    fn test_save_overwrites_previous() {
        let mut store = SqliteStore::in_memory().unwrap();

        // Build initial graph with just "add" function (4 nodes, 3 edges)
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();
        let i32_id = TypeId::I32;

        let add_fn = graph
            .add_function(
                "add".into(),
                root,
                vec![("a".into(), i32_id), ("b".into(), i32_id)],
                i32_id,
                Visibility::Public,
            )
            .unwrap();

        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, add_fn)
            .unwrap();
        let param_b = graph
            .add_core_op(ComputeOp::Parameter { index: 1 }, add_fn)
            .unwrap();
        let sum = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, add_fn)
            .unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, add_fn).unwrap();

        graph.add_data_edge(param_a, sum, 0, 0, i32_id).unwrap();
        graph.add_data_edge(param_b, sum, 0, 1, i32_id).unwrap();
        graph.add_data_edge(sum, ret, 0, 0, i32_id).unwrap();

        let id = store.create_program("overwrite_test").unwrap();
        store.save_program(id, &graph).unwrap();

        // Verify initial save
        let loaded1 = store.load_program(id).unwrap();
        assert_eq!(loaded1.node_count(), 4);
        assert_eq!(loaded1.edge_count(), 3);
        assert_eq!(loaded1.function_count(), 1);

        // Now add a second function to the graph and save again
        let neg_fn = graph
            .add_function(
                "negate".into(),
                root,
                vec![("x".into(), i32_id)],
                i32_id,
                Visibility::Public,
            )
            .unwrap();

        let param_x = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, neg_fn)
            .unwrap();
        let neg = graph
            .add_core_op(
                ComputeOp::UnaryArith {
                    op: UnaryArithOp::Neg,
                },
                neg_fn,
            )
            .unwrap();
        let neg_ret = graph.add_core_op(ComputeOp::Return, neg_fn).unwrap();

        graph.add_data_edge(param_x, neg, 0, 0, i32_id).unwrap();
        graph.add_data_edge(neg, neg_ret, 0, 0, i32_id).unwrap();

        // Overwrite with new graph
        store.save_program(id, &graph).unwrap();

        // Load and verify the modification is present
        let loaded2 = store.load_program(id).unwrap();
        assert_eq!(loaded2.node_count(), 7); // 4 + 3
        assert_eq!(loaded2.edge_count(), 5); // 3 + 2
        assert_eq!(loaded2.function_count(), 2); // add + negate
    }
}

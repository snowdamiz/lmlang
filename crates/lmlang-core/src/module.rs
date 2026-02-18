//! Module tree for hierarchical program organization.
//!
//! [`ModuleDef`] represents a single module, and [`ModuleTree`] manages the
//! full hierarchy. Modules follow Rust's `mod` system: they form a tree with
//! a root module, support pub/private visibility, and track which functions
//! and type definitions belong to each module.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::id::{FunctionId, ModuleId};
use crate::type_id::TypeId;
use crate::types::Visibility;

/// A module definition within the module tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleDef {
    /// Unique identity for this module.
    pub id: ModuleId,
    /// Module name.
    pub name: String,
    /// Parent module. `None` for the root module.
    pub parent: Option<ModuleId>,
    /// Visibility across module boundaries (pub/private).
    pub visibility: Visibility,
}

/// Manages the hierarchical module tree.
///
/// Tracks parent/child relationships between modules, and which functions
/// and type definitions belong to each module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleTree {
    /// All modules indexed by their ID.
    modules: HashMap<ModuleId, ModuleDef>,
    /// Parent -> children mapping.
    children: HashMap<ModuleId, Vec<ModuleId>>,
    /// Module -> functions mapping.
    functions: HashMap<ModuleId, Vec<FunctionId>>,
    /// Module -> type definitions mapping.
    type_defs: HashMap<ModuleId, Vec<TypeId>>,
    /// The root module ID.
    root: ModuleId,
    /// Counter for generating the next ModuleId.
    next_id: u32,
}

impl ModuleTree {
    /// Creates a new module tree with a root module.
    ///
    /// The root module gets `ModuleId(0)` and is always public.
    pub fn new(root_name: &str) -> Self {
        let root_id = ModuleId(0);
        let root = ModuleDef {
            id: root_id,
            name: root_name.to_string(),
            parent: None,
            visibility: Visibility::Public,
        };

        let mut modules = HashMap::new();
        modules.insert(root_id, root);

        let mut children = HashMap::new();
        children.insert(root_id, Vec::new());

        let mut functions = HashMap::new();
        functions.insert(root_id, Vec::new());

        let mut type_defs = HashMap::new();
        type_defs.insert(root_id, Vec::new());

        ModuleTree {
            modules,
            children,
            functions,
            type_defs,
            root: root_id,
            next_id: 1,
        }
    }

    /// Reconstructs a ModuleTree from stored parts.
    ///
    /// Used by the storage layer to rebuild the tree from loaded data.
    pub fn from_parts(
        modules: HashMap<ModuleId, ModuleDef>,
        children: HashMap<ModuleId, Vec<ModuleId>>,
        functions: HashMap<ModuleId, Vec<FunctionId>>,
        type_defs: HashMap<ModuleId, Vec<TypeId>>,
        root: ModuleId,
        next_id: u32,
    ) -> Self {
        ModuleTree {
            modules,
            children,
            functions,
            type_defs,
            root,
            next_id,
        }
    }

    /// Returns the root module ID.
    pub fn root_id(&self) -> ModuleId {
        self.root
    }

    /// Creates a child module under `parent` and returns the new module's ID.
    ///
    /// Returns [`CoreError::ModuleNotFound`] if the parent does not exist.
    pub fn add_module(
        &mut self,
        name: String,
        parent: ModuleId,
        visibility: Visibility,
    ) -> Result<ModuleId, CoreError> {
        if !self.modules.contains_key(&parent) {
            return Err(CoreError::ModuleNotFound { id: parent });
        }

        let id = ModuleId(self.next_id);
        self.next_id += 1;

        let module = ModuleDef {
            id,
            name,
            parent: Some(parent),
            visibility,
        };

        self.modules.insert(id, module);
        self.children.entry(parent).or_default().push(id);
        self.children.insert(id, Vec::new());
        self.functions.insert(id, Vec::new());
        self.type_defs.insert(id, Vec::new());

        Ok(id)
    }

    /// Looks up a module by its ID.
    pub fn get_module(&self, id: ModuleId) -> Option<&ModuleDef> {
        self.modules.get(&id)
    }

    /// Returns the child module IDs of the given module.
    ///
    /// Returns an empty slice if the module has no children or does not exist.
    pub fn children(&self, id: ModuleId) -> &[ModuleId] {
        self.children
            .get(&id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Registers a function in a module.
    ///
    /// Returns [`CoreError::ModuleNotFound`] if the module does not exist.
    pub fn add_function(
        &mut self,
        module: ModuleId,
        function: FunctionId,
    ) -> Result<(), CoreError> {
        if !self.modules.contains_key(&module) {
            return Err(CoreError::ModuleNotFound { id: module });
        }
        self.functions.entry(module).or_default().push(function);
        Ok(())
    }

    /// Registers a type definition in a module.
    ///
    /// Returns [`CoreError::ModuleNotFound`] if the module does not exist.
    pub fn add_type_def(
        &mut self,
        module: ModuleId,
        type_id: TypeId,
    ) -> Result<(), CoreError> {
        if !self.modules.contains_key(&module) {
            return Err(CoreError::ModuleNotFound { id: module });
        }
        self.type_defs.entry(module).or_default().push(type_id);
        Ok(())
    }

    /// Returns the functions registered in a module.
    ///
    /// Returns an empty slice if the module has no functions or does not exist.
    pub fn functions_in(&self, module: ModuleId) -> &[FunctionId] {
        self.functions
            .get(&module)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Returns an iterator over all modules in the tree.
    pub fn all_modules(&self) -> impl Iterator<Item = (&ModuleId, &ModuleDef)> {
        self.modules.iter()
    }

    /// Returns the parent-to-children mapping.
    pub fn children_map(&self) -> &HashMap<ModuleId, Vec<ModuleId>> {
        &self.children
    }

    /// Returns the module-to-functions mapping.
    pub fn functions_map(&self) -> &HashMap<ModuleId, Vec<FunctionId>> {
        &self.functions
    }

    /// Returns the module-to-type-definitions mapping.
    pub fn type_defs_map(&self) -> &HashMap<ModuleId, Vec<TypeId>> {
        &self.type_defs
    }

    /// Returns the next module ID counter value.
    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    /// Returns the full path from root to the given module.
    ///
    /// For example, `["root", "math", "trig"]` for a module `trig` inside
    /// `math` inside the root module.
    ///
    /// Returns an empty vec if the module does not exist.
    pub fn path(&self, id: ModuleId) -> Vec<String> {
        let mut parts = Vec::new();
        let mut current = id;

        loop {
            match self.modules.get(&current) {
                Some(m) => {
                    parts.push(m.name.clone());
                    match m.parent {
                        Some(parent) => current = parent,
                        None => break,
                    }
                }
                None => return Vec::new(),
            }
        }

        parts.reverse();
        parts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tree_has_root() {
        let tree = ModuleTree::new("crate");
        let root = tree.get_module(tree.root_id()).unwrap();
        assert_eq!(root.name, "crate");
        assert!(root.parent.is_none());
        assert_eq!(root.visibility, Visibility::Public);
        assert_eq!(root.id, ModuleId(0));
    }

    #[test]
    fn add_child_modules() {
        let mut tree = ModuleTree::new("root");
        let root = tree.root_id();

        let math = tree
            .add_module("math".into(), root, Visibility::Public)
            .unwrap();
        let trig = tree
            .add_module("trig".into(), math, Visibility::Private)
            .unwrap();

        assert_eq!(tree.children(root), &[math]);
        assert_eq!(tree.children(math), &[trig]);
        assert!(tree.children(trig).is_empty());

        let trig_def = tree.get_module(trig).unwrap();
        assert_eq!(trig_def.parent, Some(math));
        assert_eq!(trig_def.visibility, Visibility::Private);
    }

    #[test]
    fn path_from_root() {
        let mut tree = ModuleTree::new("root");
        let root = tree.root_id();

        let math = tree
            .add_module("math".into(), root, Visibility::Public)
            .unwrap();
        let trig = tree
            .add_module("trig".into(), math, Visibility::Public)
            .unwrap();

        assert_eq!(tree.path(root), vec!["root"]);
        assert_eq!(tree.path(math), vec!["root", "math"]);
        assert_eq!(tree.path(trig), vec!["root", "math", "trig"]);
    }

    #[test]
    fn path_nonexistent_module() {
        let tree = ModuleTree::new("root");
        assert!(tree.path(ModuleId(999)).is_empty());
    }

    #[test]
    fn add_function_to_module() {
        let mut tree = ModuleTree::new("root");
        let root = tree.root_id();

        tree.add_function(root, FunctionId(1)).unwrap();
        tree.add_function(root, FunctionId(2)).unwrap();

        assert_eq!(tree.functions_in(root), &[FunctionId(1), FunctionId(2)]);
    }

    #[test]
    fn add_type_def_to_module() {
        let mut tree = ModuleTree::new("root");
        let root = tree.root_id();

        tree.add_type_def(root, TypeId(100)).unwrap();
        assert_eq!(
            tree.type_defs.get(&root).unwrap(),
            &[TypeId(100)]
        );
    }

    #[test]
    fn add_module_to_nonexistent_parent_errors() {
        let mut tree = ModuleTree::new("root");
        let result = tree.add_module("orphan".into(), ModuleId(999), Visibility::Public);
        assert!(result.is_err());
        match result {
            Err(CoreError::ModuleNotFound { id }) => assert_eq!(id, ModuleId(999)),
            _ => panic!("expected ModuleNotFound error"),
        }
    }

    #[test]
    fn add_function_to_nonexistent_module_errors() {
        let mut tree = ModuleTree::new("root");
        let result = tree.add_function(ModuleId(999), FunctionId(1));
        assert!(result.is_err());
    }

    #[test]
    fn add_type_def_to_nonexistent_module_errors() {
        let mut tree = ModuleTree::new("root");
        let result = tree.add_type_def(ModuleId(999), TypeId(1));
        assert!(result.is_err());
    }

    #[test]
    fn root_module_has_no_parent() {
        let tree = ModuleTree::new("main");
        let root = tree.get_module(tree.root_id()).unwrap();
        assert!(root.parent.is_none());
    }

    #[test]
    fn serde_roundtrip_module_tree() {
        let mut tree = ModuleTree::new("root");
        let root = tree.root_id();
        let math = tree
            .add_module("math".into(), root, Visibility::Public)
            .unwrap();
        tree.add_function(math, FunctionId(1)).unwrap();
        tree.add_type_def(root, TypeId(50)).unwrap();

        let json = serde_json::to_string(&tree).unwrap();
        let back: ModuleTree = serde_json::from_str(&json).unwrap();

        // Compare structurally instead of by JSON string (HashMap key order is
        // non-deterministic).
        assert_eq!(back.root_id(), tree.root_id());
        assert_eq!(
            back.get_module(root).unwrap().name,
            tree.get_module(root).unwrap().name,
        );
        assert_eq!(
            back.get_module(math).unwrap().name,
            tree.get_module(math).unwrap().name,
        );
        assert_eq!(back.children(root), tree.children(root));
        assert_eq!(back.functions_in(math), tree.functions_in(math));
        assert_eq!(back.path(math), tree.path(math));
    }
}

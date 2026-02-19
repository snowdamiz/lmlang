//! Incremental compilation engine with function-level dirty tracking.
//!
//! Tracks per-function compilation hashes, constructs call graphs for
//! dependent identification, and produces recompilation plans that minimize
//! the set of functions needing recompilation after an edit.
//!
//! # Architecture
//!
//! - [`IncrementalState`]: Persistent state tracking last-compiled hashes,
//!   settings hash, and cache directory for per-function object files.
//! - [`RecompilationPlan`]: The computed plan showing dirty, dependent, and
//!   cached functions.
//! - [`build_call_graph`]: Extracts caller->callee relationships from Call
//!   nodes in the program graph.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::FunctionId;
use lmlang_core::ops::ComputeOp;
use petgraph::graph::NodeIndex;

use crate::CompileOptions;

/// Tracks compilation state for incremental builds.
///
/// Persists per-function hashes from the last successful compilation so
/// that subsequent compilations can detect which functions changed. Also
/// tracks the compilation settings hash to invalidate the entire cache
/// when settings change (e.g., optimization level or target triple).
#[derive(Debug, Serialize, Deserialize)]
pub struct IncrementalState {
    /// Per-function hash from last successful compilation.
    /// Uses `[u8; 32]` for blake3 hash portability.
    last_compiled_hashes: HashMap<FunctionId, [u8; 32]>,
    /// Compilation settings hash (opt level, target triple, debug flag).
    /// Used to detect settings changes that invalidate the entire cache.
    settings_hash: [u8; 32],
    /// Directory containing cached per-function object files.
    cache_dir: PathBuf,
}

/// A recompilation plan computed from dirty analysis.
///
/// Categorizes all functions into three groups: directly dirty (content
/// changed), dirty dependents (a callee changed), and cached (unchanged,
/// can reuse .o files).
#[derive(Debug, Clone, Serialize)]
pub struct RecompilationPlan {
    /// Functions that changed directly (content hash differs).
    pub dirty: Vec<FunctionId>,
    /// Functions dirty because a callee changed (transitive callers).
    pub dirty_dependents: Vec<FunctionId>,
    /// Functions that can use cached object files.
    pub cached: Vec<FunctionId>,
    /// Whether any recompilation is needed.
    pub needs_recompilation: bool,
}

impl IncrementalState {
    /// Create a new empty incremental state with no previous hashes.
    ///
    /// The first compilation with this state will be a full rebuild.
    pub fn new(cache_dir: PathBuf) -> Self {
        IncrementalState {
            last_compiled_hashes: HashMap::new(),
            settings_hash: [0u8; 32],
            cache_dir,
        }
    }

    /// Returns a reference to the cache directory.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Compute which functions need recompilation based on hash changes.
    ///
    /// Phase 1: Compare current hashes against last_compiled_hashes to find
    ///   directly dirty functions (changed or new).
    /// Phase 2: Build reverse call graph. BFS from dirty functions through
    ///   callers to find transitive dependents.
    /// Phase 3: Everything else is cached.
    pub fn compute_dirty(
        &self,
        current_hashes: &HashMap<FunctionId, [u8; 32]>,
        call_graph: &HashMap<FunctionId, Vec<FunctionId>>,
    ) -> RecompilationPlan {
        // Phase 1: Find directly dirty functions
        let mut directly_dirty: HashSet<FunctionId> = HashSet::new();

        for (&func_id, current_hash) in current_hashes {
            match self.last_compiled_hashes.get(&func_id) {
                Some(prev_hash) if prev_hash == current_hash => {
                    // Unchanged
                }
                _ => {
                    // New or changed
                    directly_dirty.insert(func_id);
                }
            }
        }

        // Phase 2: Build reverse call graph (callee -> callers)
        let mut reverse_graph: HashMap<FunctionId, Vec<FunctionId>> = HashMap::new();
        for (&caller, callees) in call_graph {
            for &callee in callees {
                reverse_graph.entry(callee).or_default().push(caller);
            }
        }

        // BFS from dirty functions through callers
        let mut dirty_dependents: HashSet<FunctionId> = HashSet::new();
        let mut queue: VecDeque<FunctionId> = directly_dirty.iter().copied().collect();

        while let Some(func_id) = queue.pop_front() {
            if let Some(callers) = reverse_graph.get(&func_id) {
                for &caller in callers {
                    // Only add as dependent if not already directly dirty
                    if !directly_dirty.contains(&caller) && dirty_dependents.insert(caller) {
                        queue.push_back(caller);
                    }
                }
            }
        }

        // Phase 3: Everything else is cached
        let all_dirty: HashSet<FunctionId> =
            directly_dirty.union(&dirty_dependents).copied().collect();
        let cached: Vec<FunctionId> = current_hashes
            .keys()
            .filter(|id| !all_dirty.contains(id))
            .copied()
            .collect();

        let needs_recompilation = !directly_dirty.is_empty() || !dirty_dependents.is_empty();

        // Sort for deterministic output
        let mut dirty: Vec<FunctionId> = directly_dirty.into_iter().collect();
        dirty.sort_by_key(|f| f.0);
        let mut deps: Vec<FunctionId> = dirty_dependents.into_iter().collect();
        deps.sort_by_key(|f| f.0);
        let mut cached_sorted = cached;
        cached_sorted.sort_by_key(|f| f.0);

        RecompilationPlan {
            dirty,
            dirty_dependents: deps,
            cached: cached_sorted,
            needs_recompilation,
        }
    }

    /// Update last_compiled_hashes after successful compilation.
    pub fn update_hashes(&mut self, hashes: HashMap<FunctionId, [u8; 32]>) {
        self.last_compiled_hashes = hashes;
    }

    /// Return the path to a cached .o file for a given function.
    pub fn cached_object_path(&self, func_id: FunctionId) -> PathBuf {
        self.cache_dir.join(format!("func_{}.o", func_id.0))
    }

    /// Check if compilation settings changed (invalidates entire cache).
    pub fn is_settings_changed(&self, options: &CompileOptions) -> bool {
        let current = compute_settings_hash(options);
        self.settings_hash != current
    }

    /// Update the settings hash after a compilation with new settings.
    pub fn update_settings_hash(&mut self, options: &CompileOptions) {
        self.settings_hash = compute_settings_hash(options);
    }

    /// Returns a reference to last compiled hashes.
    pub fn last_compiled_hashes(&self) -> &HashMap<FunctionId, [u8; 32]> {
        &self.last_compiled_hashes
    }

    /// Save incremental state to a JSON file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Load incremental state from a JSON file.
    ///
    /// Returns `None` if the file doesn't exist or can't be parsed.
    pub fn load(path: &Path) -> Option<Self> {
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }
}

/// Build a call graph from the program graph.
///
/// Scans all functions' nodes for `Call { target }` ops and returns
/// a map of caller -> list of callees.
pub fn build_call_graph(graph: &ProgramGraph) -> HashMap<FunctionId, Vec<FunctionId>> {
    let mut call_graph: HashMap<FunctionId, Vec<FunctionId>> = HashMap::new();

    for &func_id in graph.functions().keys() {
        let mut callees: Vec<FunctionId> = Vec::new();
        let func_nodes = graph.function_nodes(func_id);

        for node_id in func_nodes {
            let node_idx: NodeIndex<u32> = node_id.into();
            if let Some(node) = graph.compute().node_weight(node_idx) {
                if let lmlang_core::ops::ComputeNodeOp::Core(ComputeOp::Call { target }) = &node.op
                {
                    if !callees.contains(target) {
                        callees.push(*target);
                    }
                }
            }
        }

        callees.sort_by_key(|f| f.0);
        call_graph.insert(func_id, callees);
    }

    call_graph
}

/// Compute a hash of the compilation settings.
///
/// Used to detect when settings change (e.g., optimization level, target
/// triple, debug flag), which invalidates the entire object file cache.
pub fn compute_settings_hash(options: &CompileOptions) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    // Hash optimization level
    let opt_byte = match options.opt_level {
        crate::OptLevel::O0 => 0u8,
        crate::OptLevel::O1 => 1u8,
        crate::OptLevel::O2 => 2u8,
        crate::OptLevel::O3 => 3u8,
    };
    hasher.update(&[opt_byte]);
    // Hash target triple
    if let Some(ref triple) = options.target_triple {
        hasher.update(triple.as_bytes());
    } else {
        hasher.update(b"native");
    }
    // Hash debug flag
    hasher.update(&[options.debug_symbols as u8]);
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::id::FunctionId;
    use lmlang_core::ops::{ArithOp, ComputeOp};
    use lmlang_core::type_id::TypeId;
    use lmlang_core::types::Visibility;
    use lmlang_storage::hash::hash_all_functions_for_compilation;

    /// Helper: build a program with A calls B, B calls C.
    fn build_call_chain_graph() -> (ProgramGraph, FunctionId, FunctionId, FunctionId) {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        // Function C: identity function (x -> x)
        let fn_c = graph
            .add_function(
                "fn_c".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();
        let pc = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, fn_c)
            .unwrap();
        let retc = graph.add_core_op(ComputeOp::Return, fn_c).unwrap();
        graph.add_data_edge(pc, retc, 0, 0, TypeId::I32).unwrap();

        // Function B: calls C
        let fn_b = graph
            .add_function(
                "fn_b".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();
        let pb = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, fn_b)
            .unwrap();
        let call_c = graph
            .add_core_op(ComputeOp::Call { target: fn_c }, fn_b)
            .unwrap();
        let retb = graph.add_core_op(ComputeOp::Return, fn_b).unwrap();
        graph.add_data_edge(pb, call_c, 0, 0, TypeId::I32).unwrap();
        graph
            .add_data_edge(call_c, retb, 0, 0, TypeId::I32)
            .unwrap();

        // Function A: calls B
        let fn_a = graph
            .add_function(
                "fn_a".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();
        let pa = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, fn_a)
            .unwrap();
        let call_b = graph
            .add_core_op(ComputeOp::Call { target: fn_b }, fn_a)
            .unwrap();
        let reta = graph.add_core_op(ComputeOp::Return, fn_a).unwrap();
        graph.add_data_edge(pa, call_b, 0, 0, TypeId::I32).unwrap();
        graph
            .add_data_edge(call_b, reta, 0, 0, TypeId::I32)
            .unwrap();

        (graph, fn_a, fn_b, fn_c)
    }

    #[test]
    fn test_build_call_graph_correct() {
        let (graph, fn_a, fn_b, fn_c) = build_call_chain_graph();
        let cg = build_call_graph(&graph);

        // A calls B
        assert_eq!(cg[&fn_a], vec![fn_b]);
        // B calls C
        assert_eq!(cg[&fn_b], vec![fn_c]);
        // C calls nothing
        assert!(cg[&fn_c].is_empty());
    }

    #[test]
    fn test_compute_dirty_with_changed_function() {
        let (graph, fn_a, fn_b, fn_c) = build_call_chain_graph();
        let call_graph = build_call_graph(&graph);

        // Get hashes and create state with them
        let blake_hashes = hash_all_functions_for_compilation(&graph);
        let hashes: HashMap<FunctionId, [u8; 32]> = blake_hashes
            .iter()
            .map(|(&fid, h)| (fid, *h.as_bytes()))
            .collect();

        let mut state = IncrementalState::new(PathBuf::from("/tmp/test_cache"));
        state.update_hashes(hashes);

        // Now simulate changing fn_c by modifying its hash
        let mut new_hashes = state.last_compiled_hashes().clone();
        // Flip a bit in fn_c's hash to simulate a change
        new_hashes.get_mut(&fn_c).unwrap()[0] ^= 0xFF;

        let plan = state.compute_dirty(&new_hashes, &call_graph);

        // fn_c is directly dirty
        assert!(plan.dirty.contains(&fn_c));
        assert!(!plan.dirty.contains(&fn_a));
        assert!(!plan.dirty.contains(&fn_b));

        // fn_b is a dirty dependent (calls fn_c)
        assert!(plan.dirty_dependents.contains(&fn_b));
        // fn_a is a dirty dependent (calls fn_b which calls fn_c)
        assert!(plan.dirty_dependents.contains(&fn_a));

        // No functions are cached
        assert!(plan.cached.is_empty());
        assert!(plan.needs_recompilation);
    }

    #[test]
    fn test_compute_dirty_no_changes() {
        let (graph, _fn_a, _fn_b, _fn_c) = build_call_chain_graph();
        let call_graph = build_call_graph(&graph);

        let blake_hashes = hash_all_functions_for_compilation(&graph);
        let hashes: HashMap<FunctionId, [u8; 32]> = blake_hashes
            .iter()
            .map(|(&fid, h)| (fid, *h.as_bytes()))
            .collect();

        let mut state = IncrementalState::new(PathBuf::from("/tmp/test_cache"));
        state.update_hashes(hashes.clone());

        let plan = state.compute_dirty(&hashes, &call_graph);

        assert!(plan.dirty.is_empty());
        assert!(plan.dirty_dependents.is_empty());
        assert_eq!(plan.cached.len(), 3);
        assert!(!plan.needs_recompilation);
    }

    #[test]
    fn test_compute_dirty_leaf_change_only_affects_callers() {
        // Build a graph: A calls B, A calls C (independent), C calls nothing
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let fn_c = graph
            .add_function(
                "fn_c".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();
        let pc = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, fn_c)
            .unwrap();
        let retc = graph.add_core_op(ComputeOp::Return, fn_c).unwrap();
        graph.add_data_edge(pc, retc, 0, 0, TypeId::I32).unwrap();

        let fn_b = graph
            .add_function(
                "fn_b".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();
        let pb = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, fn_b)
            .unwrap();
        let retb = graph.add_core_op(ComputeOp::Return, fn_b).unwrap();
        graph.add_data_edge(pb, retb, 0, 0, TypeId::I32).unwrap();

        // A calls both B and C
        let fn_a = graph
            .add_function(
                "fn_a".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();
        let pa = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, fn_a)
            .unwrap();
        let call_b = graph
            .add_core_op(ComputeOp::Call { target: fn_b }, fn_a)
            .unwrap();
        let call_c = graph
            .add_core_op(ComputeOp::Call { target: fn_c }, fn_a)
            .unwrap();
        let add = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, fn_a)
            .unwrap();
        let reta = graph.add_core_op(ComputeOp::Return, fn_a).unwrap();
        graph.add_data_edge(pa, call_b, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(pa, call_c, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(call_b, add, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(call_c, add, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(add, reta, 0, 0, TypeId::I32).unwrap();

        let call_graph = build_call_graph(&graph);
        let blake_hashes = hash_all_functions_for_compilation(&graph);
        let hashes: HashMap<FunctionId, [u8; 32]> = blake_hashes
            .iter()
            .map(|(&fid, h)| (fid, *h.as_bytes()))
            .collect();

        let mut state = IncrementalState::new(PathBuf::from("/tmp/test_cache"));
        state.update_hashes(hashes);

        // Change only fn_b
        let mut new_hashes = state.last_compiled_hashes().clone();
        new_hashes.get_mut(&fn_b).unwrap()[0] ^= 0xFF;

        let plan = state.compute_dirty(&new_hashes, &call_graph);

        // fn_b is directly dirty
        assert!(plan.dirty.contains(&fn_b));
        // fn_a is dependent (calls fn_b)
        assert!(plan.dirty_dependents.contains(&fn_a));
        // fn_c is cached (not affected by fn_b change)
        assert!(plan.cached.contains(&fn_c));
        assert!(!plan.dirty.contains(&fn_c));
        assert!(!plan.dirty_dependents.contains(&fn_c));
    }

    #[test]
    fn test_contract_changes_do_not_dirty() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "f".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let param = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(param, ret, 0, 0, TypeId::I32).unwrap();

        // Get hashes BEFORE adding contract
        let blake_hashes = hash_all_functions_for_compilation(&graph);
        let hashes: HashMap<FunctionId, [u8; 32]> = blake_hashes
            .iter()
            .map(|(&fid, h)| (fid, *h.as_bytes()))
            .collect();

        let mut state = IncrementalState::new(PathBuf::from("/tmp/test_cache"));
        state.update_hashes(hashes);

        // Add a contract node
        let _precond = graph
            .add_core_op(
                ComputeOp::Precondition {
                    message: "x > 0".into(),
                },
                func_id,
            )
            .unwrap();

        // Get new hashes (using compilation hash which excludes contracts)
        let new_blake_hashes = hash_all_functions_for_compilation(&graph);
        let new_hashes: HashMap<FunctionId, [u8; 32]> = new_blake_hashes
            .iter()
            .map(|(&fid, h)| (fid, *h.as_bytes()))
            .collect();

        let call_graph = build_call_graph(&graph);
        let plan = state.compute_dirty(&new_hashes, &call_graph);

        // Should NOT be dirty -- contract changes excluded from compilation hash
        assert!(plan.dirty.is_empty());
        assert!(plan.dirty_dependents.is_empty());
        assert!(!plan.needs_recompilation);
    }

    #[test]
    fn test_settings_hash_changes() {
        let opts1 = CompileOptions {
            opt_level: crate::OptLevel::O0,
            ..Default::default()
        };
        let opts2 = CompileOptions {
            opt_level: crate::OptLevel::O2,
            ..Default::default()
        };

        let mut state = IncrementalState::new(PathBuf::from("/tmp/test_cache"));
        state.update_settings_hash(&opts1);

        assert!(!state.is_settings_changed(&opts1));
        assert!(state.is_settings_changed(&opts2));
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let state_path = temp_dir.path().join("state.json");

        let mut state = IncrementalState::new(temp_dir.path().to_path_buf());
        let mut hashes = HashMap::new();
        hashes.insert(FunctionId(0), [42u8; 32]);
        hashes.insert(FunctionId(1), [99u8; 32]);
        state.update_hashes(hashes);
        state.update_settings_hash(&CompileOptions::default());

        state.save(&state_path).unwrap();
        let loaded = IncrementalState::load(&state_path).unwrap();

        assert_eq!(loaded.last_compiled_hashes().len(), 2);
        assert_eq!(loaded.last_compiled_hashes()[&FunctionId(0)], [42u8; 32]);
        assert_eq!(loaded.last_compiled_hashes()[&FunctionId(1)], [99u8; 32]);
        assert_eq!(loaded.settings_hash, state.settings_hash);
    }

    #[test]
    fn test_cached_object_path() {
        let state = IncrementalState::new(PathBuf::from("/tmp/cache"));
        assert_eq!(
            state.cached_object_path(FunctionId(42)),
            PathBuf::from("/tmp/cache/func_42.o")
        );
    }
}

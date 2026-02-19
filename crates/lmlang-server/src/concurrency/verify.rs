//! Incremental verification helpers for agent-aware mutations.

use std::collections::{HashMap, HashSet, VecDeque};

use lmlang_check::typecheck::{self, TypeError};
use lmlang_codegen::incremental::build_call_graph;
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::FunctionId;

use crate::schema::diagnostics::DiagnosticError;
use crate::schema::verify::VerifyResponse;

/// Builds verification scope = changed functions + transitive callers.
pub fn find_verification_scope(
    graph: &ProgramGraph,
    affected_functions: &[FunctionId],
) -> Vec<FunctionId> {
    if affected_functions.is_empty() {
        return Vec::new();
    }

    let call_graph = build_call_graph(graph);
    let mut reverse: HashMap<FunctionId, Vec<FunctionId>> = HashMap::new();
    for (caller, callees) in call_graph {
        for callee in callees {
            reverse.entry(callee).or_default().push(caller);
        }
    }

    let mut seen: HashSet<FunctionId> = HashSet::new();
    let mut queue: VecDeque<FunctionId> = affected_functions.iter().copied().collect();

    while let Some(func_id) = queue.pop_front() {
        if !seen.insert(func_id) {
            continue;
        }

        if let Some(callers) = reverse.get(&func_id) {
            for &caller in callers {
                queue.push_back(caller);
            }
        }
    }

    let mut scope: Vec<FunctionId> = seen.into_iter().collect();
    scope.sort_by_key(|f| f.0);
    scope
}

/// Validates only functions inside the scope.
pub fn validate_functions(graph: &ProgramGraph, scope: &[FunctionId]) -> Vec<TypeError> {
    if scope.is_empty() {
        return Vec::new();
    }

    let scope_set: HashSet<FunctionId> = scope.iter().copied().collect();
    typecheck::validate_graph(graph)
        .into_iter()
        .filter(|err| error_function_id(err).is_some_and(|f| scope_set.contains(&f)))
        .collect()
}

/// Runs incremental verification for affected functions and their dependents.
pub fn run_incremental_verification(
    graph: &ProgramGraph,
    affected_functions: &[FunctionId],
) -> VerifyResponse {
    let scope = find_verification_scope(graph, affected_functions);
    let errors = validate_functions(graph, &scope)
        .into_iter()
        .map(DiagnosticError::from)
        .collect::<Vec<_>>();

    VerifyResponse {
        valid: errors.is_empty(),
        errors,
        warnings: Vec::new(),
    }
}

fn error_function_id(err: &TypeError) -> Option<FunctionId> {
    match err {
        TypeError::TypeMismatch { function_id, .. }
        | TypeError::MissingInput { function_id, .. }
        | TypeError::WrongInputCount { function_id, .. }
        | TypeError::NonNumericArithmetic { function_id, .. }
        | TypeError::NonBooleanCondition { function_id, .. } => Some(*function_id),
        TypeError::UnknownType { .. } => None,
    }
}

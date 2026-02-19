//! Deterministic planner-action executor for autonomous program-building runs.
//!
//! The executor consumes a validated planner envelope and dispatches each
//! action through existing `ProgramService` primitives. It normalizes success
//! and failures into typed execution evidence for transcript/API projection.
#![allow(clippy::result_large_err)]

use serde::Serialize;

use crate::error::ApiError;
use crate::schema::autonomy_execution::{
    AutonomyActionExecutionResult, AutonomyActionStatus, AutonomyDiagnostics,
    AutonomyDiagnosticsClass, AutonomyExecutionAttemptSummary, AutonomyExecutionError,
    AutonomyExecutionErrorCode, AutonomyExecutionOutcome, AutonomyExecutionStatus, StopReason,
    StopReasonCode,
};
use crate::schema::autonomy_plan::{
    AutonomyPlanAction, AutonomyPlanCompileRequest, AutonomyPlanEnvelope,
    AutonomyPlanHistoryOperation, AutonomyPlanHistoryRequest, AutonomyPlanInspectRequest,
    AutonomyPlanMutationRequest, AutonomyPlanSimulateRequest, AutonomyPlanVerifyRequest,
};
use crate::schema::compile::CompileRequest;
use crate::schema::queries::{DetailLevel, SearchRequest};
use crate::schema::simulate::SimulateRequest;
use crate::schema::verify::VerifyResponse;
use crate::service::ProgramService;

/// Execute all actions in one validated planner envelope.
///
/// Execution is fail-fast: the first failed action returns a terminal failed
/// outcome with explicit stop reason and action-level error classification.
pub fn execute_plan(
    service: &mut ProgramService,
    envelope: &AutonomyPlanEnvelope,
) -> AutonomyExecutionOutcome {
    let mut action_results = Vec::with_capacity(envelope.actions.len());

    for (action_index, action) in envelope.actions.iter().enumerate() {
        match execute_action(service, action_index, action) {
            Ok(action_result) => action_results.push(action_result),
            Err(action_result) => {
                action_results.push(action_result.clone());
                let retryable = action_result
                    .error
                    .as_ref()
                    .map(|err| err.retryable)
                    .unwrap_or(false);

                let stop_reason = StopReason::new(
                    if retryable {
                        StopReasonCode::ActionFailedRetryable
                    } else {
                        StopReasonCode::ActionFailedNonRetryable
                    },
                    action_result.summary.clone(),
                )
                .with_detail(serde_json::json!({
                    "action_index": action_result.action_index,
                    "kind": action_result.kind,
                    "error": action_result.error,
                }));

                let attempt = build_attempt_summary(envelope, &action_results, stop_reason.clone());
                return AutonomyExecutionOutcome::from_single_attempt(
                    envelope.goal.clone(),
                    envelope.version.clone(),
                    AutonomyExecutionStatus::Failed,
                    attempt,
                    stop_reason,
                );
            }
        }
    }

    let stop_reason = StopReason::new(StopReasonCode::Completed, "All planner actions completed.");
    let attempt = build_attempt_summary(envelope, &action_results, stop_reason.clone());

    AutonomyExecutionOutcome::from_single_attempt(
        envelope.goal.clone(),
        envelope.version.clone(),
        AutonomyExecutionStatus::Succeeded,
        attempt,
        stop_reason,
    )
}

fn build_attempt_summary(
    envelope: &AutonomyPlanEnvelope,
    action_results: &[AutonomyActionExecutionResult],
    stop_reason: StopReason,
) -> AutonomyExecutionAttemptSummary {
    let succeeded_actions = action_results
        .iter()
        .filter(|result| result.status == AutonomyActionStatus::Succeeded)
        .count();

    AutonomyExecutionAttemptSummary {
        attempt: 1,
        max_attempts: 1,
        planner_status: "accepted".to_string(),
        action_count: envelope.actions.len(),
        succeeded_actions,
        action_results: action_results.to_vec(),
        stop_reason: Some(stop_reason),
    }
}

fn execute_action(
    service: &mut ProgramService,
    action_index: usize,
    action: &AutonomyPlanAction,
) -> Result<AutonomyActionExecutionResult, AutonomyActionExecutionResult> {
    match action {
        AutonomyPlanAction::MutateBatch { request, .. } => {
            execute_mutate_batch(service, action_index, request)
        }
        AutonomyPlanAction::Verify { request, .. } => {
            execute_verify(service, action_index, request)
        }
        AutonomyPlanAction::Compile { request, .. } => {
            execute_compile(service, action_index, request)
        }
        AutonomyPlanAction::Simulate { request, .. } => {
            execute_simulate(service, action_index, request)
        }
        AutonomyPlanAction::Inspect { request, .. } => {
            execute_inspect(service, action_index, request)
        }
        AutonomyPlanAction::History { request, .. } => {
            execute_history(service, action_index, request)
        }
    }
}

fn execute_mutate_batch(
    service: &mut ProgramService,
    action_index: usize,
    request: &AutonomyPlanMutationRequest,
) -> Result<AutonomyActionExecutionResult, AutonomyActionExecutionResult> {
    if request.mutations.is_empty() {
        return Err(invalid_payload_result(
            action_index,
            "mutate_batch",
            "mutate_batch request requires at least one mutation",
        ));
    }

    let response = service
        .propose_edit(request.to_propose_edit_request())
        .map_err(|err| {
            api_error_result(
                action_index,
                "mutate_batch",
                "mutation execution failed",
                err,
            )
        })?;

    if !response.valid {
        let summary = format!(
            "mutation batch rejected by validator with {} error(s)",
            response.errors.len()
        );
        let error = AutonomyExecutionError::new(
            AutonomyExecutionErrorCode::ValidationFailed,
            "mutation batch rejected by service validation",
            true,
        )
        .with_details(serde_json::to_value(&response.errors).unwrap_or(serde_json::Value::Null));
        let diagnostics = diagnostics_from_error("mutate_batch", &summary, &error);
        return Err(AutonomyActionExecutionResult::failed(
            action_index,
            "mutate_batch",
            summary,
            error.with_diagnostics(diagnostics.clone()),
        )
        .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null))
        .with_diagnostics(diagnostics));
    }

    Ok(AutonomyActionExecutionResult::succeeded(
        action_index,
        "mutate_batch",
        if request.dry_run {
            format!(
                "validated {} mutation(s) in dry-run mode",
                request.mutations.len()
            )
        } else {
            format!("applied {} mutation(s)", request.mutations.len())
        },
    )
    .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)))
}

fn execute_verify(
    service: &mut ProgramService,
    action_index: usize,
    request: &AutonomyPlanVerifyRequest,
) -> Result<AutonomyActionExecutionResult, AutonomyActionExecutionResult> {
    let Some(scope) = request.scope else {
        return Err(invalid_payload_result(
            action_index,
            "verify",
            "verify request requires `scope`",
        ));
    };

    let response = service
        .verify(scope, None)
        .map_err(|err| api_error_result(action_index, "verify", "verify action failed", err))?;

    if !response.valid {
        return Err(verify_failure_result(action_index, &response));
    }

    Ok(
        AutonomyActionExecutionResult::succeeded(action_index, "verify", "verification passed")
            .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)),
    )
}

fn execute_compile(
    service: &mut ProgramService,
    action_index: usize,
    request: &AutonomyPlanCompileRequest,
) -> Result<AutonomyActionExecutionResult, AutonomyActionExecutionResult> {
    let Some(entry_function) = request.entry_function.clone() else {
        return Err(invalid_payload_result(
            action_index,
            "compile",
            "compile request requires `entry_function`",
        ));
    };
    if entry_function.trim().is_empty() {
        return Err(invalid_payload_result(
            action_index,
            "compile",
            "compile request `entry_function` must not be empty",
        ));
    }

    let response = service
        .compile(&CompileRequest {
            opt_level: request.opt_level.clone(),
            target_triple: request.target_triple.clone(),
            debug_symbols: request.debug_symbols,
            entry_function: Some(entry_function.clone()),
            output_dir: request.output_dir.clone(),
        })
        .map_err(|err| api_error_result(action_index, "compile", "compile action failed", err))?;

    Ok(AutonomyActionExecutionResult::succeeded(
        action_index,
        "compile",
        format!(
            "compiled `{}` ({} bytes, {} ms)",
            entry_function, response.binary_size, response.compilation_time_ms
        ),
    )
    .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)))
}

fn execute_simulate(
    service: &mut ProgramService,
    action_index: usize,
    request: &AutonomyPlanSimulateRequest,
) -> Result<AutonomyActionExecutionResult, AutonomyActionExecutionResult> {
    let Some(function_id) = request.function_id else {
        return Err(invalid_payload_result(
            action_index,
            "simulate",
            "simulate request requires `function_id`",
        ));
    };

    let response = service
        .simulate(SimulateRequest {
            function_id,
            inputs: request.inputs.clone(),
            trace_enabled: request.trace_enabled,
        })
        .map_err(|err| api_error_result(action_index, "simulate", "simulate action failed", err))?;

    if !response.success {
        let error = AutonomyExecutionError::new(
            AutonomyExecutionErrorCode::ValidationFailed,
            "simulation returned success=false",
            true,
        )
        .with_details(serde_json::to_value(&response.error).unwrap_or(serde_json::Value::Null));
        let diagnostics = diagnostics_from_error(
            "simulate",
            "simulation reported unsuccessful status",
            &error,
        );
        return Err(AutonomyActionExecutionResult::failed(
            action_index,
            "simulate",
            "simulation reported unsuccessful status".to_string(),
            error.with_diagnostics(diagnostics.clone()),
        )
        .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null))
        .with_diagnostics(diagnostics));
    }

    Ok(
        AutonomyActionExecutionResult::succeeded(action_index, "simulate", "simulation completed")
            .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)),
    )
}

fn execute_inspect(
    service: &mut ProgramService,
    action_index: usize,
    request: &AutonomyPlanInspectRequest,
) -> Result<AutonomyActionExecutionResult, AutonomyActionExecutionResult> {
    if request.max_results == Some(0) {
        return Err(invalid_payload_result(
            action_index,
            "inspect",
            "inspect request `max_results` must be greater than 0 when provided",
        ));
    }

    let query = request
        .query
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty());

    match query {
        None | Some("overview") => {
            let response = service.program_overview().map_err(|err| {
                api_error_result(action_index, "inspect", "overview query failed", err)
            })?;
            Ok(AutonomyActionExecutionResult::succeeded(
                action_index,
                "inspect",
                format!(
                    "overview: {} function(s), {} node(s), {} edge(s)",
                    response.functions.len(),
                    response.node_count,
                    response.edge_count
                ),
            )
            .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)))
        }
        Some(query) if query.starts_with("semantic") => {
            let include_embeddings = query.contains("with_embeddings");
            let response = service.semantic_query(include_embeddings).map_err(|err| {
                api_error_result(action_index, "inspect", "semantic query failed", err)
            })?;
            Ok(AutonomyActionExecutionResult::succeeded(
                action_index,
                "inspect",
                format!(
                    "semantic query returned {} node(s) and {} edge(s)",
                    response.node_count, response.edge_count
                ),
            )
            .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)))
        }
        Some(query) => {
            let filter = query
                .strip_prefix("search:")
                .map(str::trim)
                .filter(|q| !q.is_empty())
                .unwrap_or(query);

            let mut response = service
                .search_nodes(SearchRequest {
                    filter_type: Some(filter.to_string()),
                    owner_function: None,
                    value_type: None,
                    detail: DetailLevel::Summary,
                })
                .map_err(|err| {
                    api_error_result(action_index, "inspect", "search query failed", err)
                })?;

            if let Some(limit) = request.max_results {
                response.nodes.truncate(limit);
            }

            Ok(AutonomyActionExecutionResult::succeeded(
                action_index,
                "inspect",
                format!("search query matched {} node(s)", response.total_count),
            )
            .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)))
        }
    }
}

fn execute_history(
    service: &mut ProgramService,
    action_index: usize,
    request: &AutonomyPlanHistoryRequest,
) -> Result<AutonomyActionExecutionResult, AutonomyActionExecutionResult> {
    let Some(operation) = request.operation.as_ref() else {
        return Err(invalid_payload_result(
            action_index,
            "history",
            "history request requires `operation`",
        ));
    };

    match operation {
        AutonomyPlanHistoryOperation::ListEntries => {
            let response = service.list_history().map_err(|err| {
                api_error_result(action_index, "history", "history listing failed", err)
            })?;
            Ok(AutonomyActionExecutionResult::succeeded(
                action_index,
                "history",
                format!("listed {} history entrie(s)", response.total),
            )
            .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)))
        }
        AutonomyPlanHistoryOperation::ListCheckpoints => {
            let response = service.list_checkpoints().map_err(|err| {
                api_error_result(action_index, "history", "checkpoint listing failed", err)
            })?;
            Ok(AutonomyActionExecutionResult::succeeded(
                action_index,
                "history",
                format!("listed {} checkpoint(s)", response.checkpoints.len()),
            )
            .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)))
        }
        AutonomyPlanHistoryOperation::Undo => {
            let response = service
                .undo()
                .map_err(|err| api_error_result(action_index, "history", "undo failed", err))?;
            Ok(AutonomyActionExecutionResult::succeeded(
                action_index,
                "history",
                format!("undo success={}", response.success),
            )
            .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)))
        }
        AutonomyPlanHistoryOperation::Redo => {
            let response = service
                .redo()
                .map_err(|err| api_error_result(action_index, "history", "redo failed", err))?;
            Ok(AutonomyActionExecutionResult::succeeded(
                action_index,
                "history",
                format!("redo success={}", response.success),
            )
            .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)))
        }
        AutonomyPlanHistoryOperation::RestoreCheckpoint => {
            let checkpoint = request
                .checkpoint
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    invalid_payload_result(
                        action_index,
                        "history",
                        "history restore operation requires `checkpoint`",
                    )
                })?;
            let response = service.restore_checkpoint(checkpoint).map_err(|err| {
                api_error_result(action_index, "history", "restore checkpoint failed", err)
            })?;
            Ok(AutonomyActionExecutionResult::succeeded(
                action_index,
                "history",
                format!("restored checkpoint `{}`", response.name),
            )
            .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)))
        }
        AutonomyPlanHistoryOperation::Diff => {
            if request.from_checkpoint.is_none() && request.to_checkpoint.is_none() {
                return Err(invalid_payload_result(
                    action_index,
                    "history",
                    "history diff operation requires at least one checkpoint reference",
                ));
            }

            let response = service
                .diff_versions(
                    request.from_checkpoint.as_deref(),
                    request.to_checkpoint.as_deref(),
                )
                .map_err(|err| api_error_result(action_index, "history", "diff failed", err))?;

            Ok(AutonomyActionExecutionResult::succeeded(
                action_index,
                "history",
                "computed history diff".to_string(),
            )
            .with_detail(serde_json::to_value(&response).unwrap_or(serde_json::Value::Null)))
        }
    }
}

fn invalid_payload_result(
    action_index: usize,
    kind: &str,
    message: &str,
) -> AutonomyActionExecutionResult {
    let error = AutonomyExecutionError::new(
        AutonomyExecutionErrorCode::InvalidActionPayload,
        message,
        false,
    );
    let diagnostics = diagnostics_from_error(kind, message, &error);
    AutonomyActionExecutionResult::failed(
        action_index,
        kind,
        message,
        error.with_diagnostics(diagnostics.clone()),
    )
    .with_diagnostics(diagnostics)
}

fn api_error_result(
    action_index: usize,
    kind: &str,
    summary: &str,
    error: ApiError,
) -> AutonomyActionExecutionResult {
    let classified = classify_api_error(error);
    let diagnostics = diagnostics_from_error(kind, summary, &classified);
    AutonomyActionExecutionResult::failed(
        action_index,
        kind,
        summary,
        classified.with_diagnostics(diagnostics.clone()),
    )
    .with_diagnostics(diagnostics)
}

fn classify_api_error(error: ApiError) -> AutonomyExecutionError {
    match error {
        ApiError::NotFound(message) => {
            AutonomyExecutionError::new(AutonomyExecutionErrorCode::NotFound, message, true)
        }
        ApiError::BadRequest(message) => {
            AutonomyExecutionError::new(AutonomyExecutionErrorCode::BadRequest, message, false)
        }
        ApiError::ValidationFailed(errors) => AutonomyExecutionError::new(
            AutonomyExecutionErrorCode::ValidationFailed,
            format!("validation failed with {} error(s)", errors.len()),
            true,
        )
        .with_details(serde_json::to_value(errors).unwrap_or(serde_json::Value::Null)),
        ApiError::InternalError(message) => {
            AutonomyExecutionError::new(AutonomyExecutionErrorCode::InternalError, message, true)
        }
        ApiError::Conflict(message) => {
            AutonomyExecutionError::new(AutonomyExecutionErrorCode::Conflict, message, true)
        }
        ApiError::ConflictWithDetails { message, details } => {
            AutonomyExecutionError::new(AutonomyExecutionErrorCode::Conflict, message, true)
                .with_details(details)
        }
        ApiError::LockDenied(denial) => AutonomyExecutionError::new(
            AutonomyExecutionErrorCode::Conflict,
            "lock denied while executing action",
            true,
        )
        .with_details(serde_json::to_value(denial).unwrap_or(serde_json::Value::Null)),
        ApiError::LockRequired(message) => {
            AutonomyExecutionError::new(AutonomyExecutionErrorCode::BadRequest, message, false)
        }
        ApiError::AgentRequired(message) => {
            AutonomyExecutionError::new(AutonomyExecutionErrorCode::BadRequest, message, false)
        }
        ApiError::TooManyRetries(message) => {
            AutonomyExecutionError::new(AutonomyExecutionErrorCode::Conflict, message, true)
        }
    }
}

fn _json_detail<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

fn verify_failure_result(
    action_index: usize,
    response: &VerifyResponse,
) -> AutonomyActionExecutionResult {
    let diagnostics = verify_failure_diagnostics(response);
    let error = AutonomyExecutionError::new(
        AutonomyExecutionErrorCode::ValidationFailed,
        "verify action reported type errors",
        true,
    )
    .with_details(serde_json::to_value(&response.errors).unwrap_or(serde_json::Value::Null))
    .with_diagnostics(diagnostics.clone());

    AutonomyActionExecutionResult::failed(
        action_index,
        "verify",
        format!(
            "verification failed with {} error(s)",
            response.errors.len()
        ),
        error,
    )
    .with_detail(serde_json::to_value(response).unwrap_or(serde_json::Value::Null))
    .with_diagnostics(diagnostics)
}

fn verify_failure_diagnostics(response: &VerifyResponse) -> AutonomyDiagnostics {
    let messages = response
        .errors
        .iter()
        .take(3)
        .map(|error| format!("[{}] {}", error.code, error.message))
        .collect::<Vec<_>>();
    let sample_codes = response
        .errors
        .iter()
        .take(5)
        .map(|error| error.code.clone())
        .collect::<Vec<_>>();
    AutonomyDiagnostics::new(
        AutonomyDiagnosticsClass::VerifyFailure,
        true,
        format!("verify reported {} diagnostic(s)", response.errors.len()),
    )
    .with_messages(messages)
    .with_detail(serde_json::json!({
        "error_count": response.errors.len(),
        "warning_count": response.warnings.len(),
        "sample_codes": sample_codes,
    }))
}

fn diagnostics_from_error(
    kind: &str,
    summary: &str,
    error: &AutonomyExecutionError,
) -> AutonomyDiagnostics {
    AutonomyDiagnostics::new(diagnostics_class_for_action(kind), error.retryable, summary)
        .with_messages(vec![error.message.clone()])
        .with_detail(serde_json::json!({
            "kind": kind,
            "error_code": error.code,
            "error_details": error.details.clone(),
        }))
}

fn diagnostics_class_for_action(kind: &str) -> AutonomyDiagnosticsClass {
    match kind {
        "verify" => AutonomyDiagnosticsClass::VerifyFailure,
        "compile" => AutonomyDiagnosticsClass::CompileFailure,
        _ => AutonomyDiagnosticsClass::ActionFailure,
    }
}

#[cfg(test)]
mod tests {
    use lmlang_core::id::ModuleId;
    use lmlang_core::type_id::TypeId;
    use lmlang_core::types::Visibility;

    use super::*;
    use crate::schema::autonomy_plan::{
        AutonomyPlanCompileRequest, AutonomyPlanHistoryRequest, AutonomyPlanInspectRequest,
        AutonomyPlanMetadata, AutonomyPlanMutationRequest, AutonomyPlanSimulateRequest,
        AutonomyPlanVerifyRequest,
    };
    use crate::schema::diagnostics::DiagnosticError;
    use crate::schema::mutations::Mutation;
    use crate::schema::verify::VerifyScope;

    fn envelope_with_actions(actions: Vec<AutonomyPlanAction>) -> AutonomyPlanEnvelope {
        AutonomyPlanEnvelope {
            version: "2026-02-19".to_string(),
            goal: "test goal".to_string(),
            metadata: AutonomyPlanMetadata::default(),
            actions,
            failure: None,
        }
    }

    #[test]
    fn execute_plan_succeeds_for_supported_actions() {
        let mut service = ProgramService::in_memory().expect("in-memory service");
        let envelope = envelope_with_actions(vec![
            AutonomyPlanAction::MutateBatch {
                request: AutonomyPlanMutationRequest {
                    mutations: vec![Mutation::AddFunction {
                        name: "main".to_string(),
                        module: ModuleId(0),
                        params: Vec::new(),
                        return_type: TypeId::UNIT,
                        visibility: Visibility::Public,
                    }],
                    dry_run: false,
                    expected_hashes: None,
                },
                rationale: None,
            },
            AutonomyPlanAction::Verify {
                request: AutonomyPlanVerifyRequest {
                    scope: Some(VerifyScope::Full),
                },
                rationale: None,
            },
            AutonomyPlanAction::Inspect {
                request: AutonomyPlanInspectRequest {
                    query: Some("overview".to_string()),
                    max_results: None,
                },
                rationale: None,
            },
            AutonomyPlanAction::History {
                request: AutonomyPlanHistoryRequest {
                    operation: Some(AutonomyPlanHistoryOperation::ListEntries),
                    checkpoint: None,
                    from_checkpoint: None,
                    to_checkpoint: None,
                    limit: None,
                },
                rationale: None,
            },
        ]);

        let outcome = execute_plan(&mut service, &envelope);
        assert_eq!(outcome.status, AutonomyExecutionStatus::Succeeded);
        assert_eq!(outcome.stop_reason.code, StopReasonCode::Completed);
        assert_eq!(outcome.attempts.len(), 1);
        assert_eq!(outcome.attempts[0].action_results.len(), 4);
        assert_eq!(outcome.attempts[0].succeeded_actions, 4);
    }

    #[test]
    fn mutate_batch_empty_mutations_is_invalid_payload() {
        let mut service = ProgramService::in_memory().expect("in-memory service");
        let envelope = envelope_with_actions(vec![AutonomyPlanAction::MutateBatch {
            request: AutonomyPlanMutationRequest {
                mutations: Vec::new(),
                dry_run: false,
                expected_hashes: None,
            },
            rationale: None,
        }]);

        let outcome = execute_plan(&mut service, &envelope);
        let action = &outcome.attempts[0].action_results[0];
        assert_eq!(outcome.status, AutonomyExecutionStatus::Failed);
        assert_eq!(
            outcome.stop_reason.code,
            StopReasonCode::ActionFailedNonRetryable
        );
        assert_eq!(action.status, AutonomyActionStatus::Failed);
        assert_eq!(
            action.error.as_ref().map(|err| err.code),
            Some(AutonomyExecutionErrorCode::InvalidActionPayload)
        );
    }

    #[test]
    fn verify_without_scope_is_invalid_payload() {
        let mut service = ProgramService::in_memory().expect("in-memory service");
        let envelope = envelope_with_actions(vec![AutonomyPlanAction::Verify {
            request: AutonomyPlanVerifyRequest { scope: None },
            rationale: None,
        }]);

        let outcome = execute_plan(&mut service, &envelope);
        let action = &outcome.attempts[0].action_results[0];
        assert_eq!(action.status, AutonomyActionStatus::Failed);
        assert_eq!(
            action.error.as_ref().map(|err| err.code),
            Some(AutonomyExecutionErrorCode::InvalidActionPayload)
        );
    }

    #[test]
    fn compile_action_failure_is_classified() {
        let mut service = ProgramService::in_memory().expect("in-memory service");
        let envelope = envelope_with_actions(vec![AutonomyPlanAction::Compile {
            request: AutonomyPlanCompileRequest {
                opt_level: "O9".to_string(),
                target_triple: None,
                debug_symbols: false,
                entry_function: Some("missing_entry".to_string()),
                output_dir: None,
            },
            rationale: None,
        }]);

        let outcome = execute_plan(&mut service, &envelope);
        let action = &outcome.attempts[0].action_results[0];
        let error = action
            .error
            .as_ref()
            .expect("compile failure has error payload");
        let action_diagnostics = action
            .diagnostics
            .as_ref()
            .expect("compile failure includes diagnostics");
        let error_diagnostics = error
            .diagnostics
            .as_ref()
            .expect("compile failure error includes diagnostics");
        assert_eq!(action.status, AutonomyActionStatus::Failed);
        assert_eq!(error.code, AutonomyExecutionErrorCode::BadRequest);
        assert_eq!(
            action_diagnostics.class,
            AutonomyDiagnosticsClass::CompileFailure
        );
        assert_eq!(action_diagnostics.retryable, error.retryable);
        assert_eq!(
            error_diagnostics.class,
            AutonomyDiagnosticsClass::CompileFailure
        );
        assert_eq!(error_diagnostics.retryable, error.retryable);
    }

    #[test]
    fn verify_failure_result_emits_deterministic_diagnostics() {
        let response = VerifyResponse {
            valid: false,
            errors: vec![
                DiagnosticError {
                    code: "TYPE_MISMATCH".to_string(),
                    message: "type mismatch at node 10".to_string(),
                    details: None,
                },
                DiagnosticError {
                    code: "MISSING_INPUT".to_string(),
                    message: "input 0 missing".to_string(),
                    details: None,
                },
            ],
            warnings: Vec::new(),
        };

        let action = verify_failure_result(3, &response);
        let error = action
            .error
            .as_ref()
            .expect("verify failure has error payload");
        let action_diagnostics = action
            .diagnostics
            .as_ref()
            .expect("verify failure has diagnostics");
        let error_diagnostics = error
            .diagnostics
            .as_ref()
            .expect("verify failure error has diagnostics");

        assert_eq!(action.status, AutonomyActionStatus::Failed);
        assert_eq!(error.code, AutonomyExecutionErrorCode::ValidationFailed);
        assert!(error.retryable);
        assert_eq!(
            action_diagnostics.class,
            AutonomyDiagnosticsClass::VerifyFailure
        );
        assert_eq!(action_diagnostics.retryable, error.retryable);
        assert_eq!(
            action_diagnostics.summary,
            "verify reported 2 diagnostic(s)"
        );
        assert_eq!(action_diagnostics.messages.len(), 2);
        assert_eq!(
            error_diagnostics.class,
            AutonomyDiagnosticsClass::VerifyFailure
        );
        assert_eq!(error_diagnostics.retryable, error.retryable);
        assert_eq!(error_diagnostics.summary, "verify reported 2 diagnostic(s)");
    }

    #[test]
    fn simulate_without_function_id_is_invalid_payload() {
        let mut service = ProgramService::in_memory().expect("in-memory service");
        let envelope = envelope_with_actions(vec![AutonomyPlanAction::Simulate {
            request: AutonomyPlanSimulateRequest {
                function_id: None,
                inputs: vec![serde_json::json!(1)],
                trace_enabled: Some(false),
            },
            rationale: None,
        }]);

        let outcome = execute_plan(&mut service, &envelope);
        let action = &outcome.attempts[0].action_results[0];
        assert_eq!(action.status, AutonomyActionStatus::Failed);
        assert_eq!(
            action.error.as_ref().map(|err| err.code),
            Some(AutonomyExecutionErrorCode::InvalidActionPayload)
        );
    }

    #[test]
    fn inspect_zero_max_results_is_invalid_payload() {
        let mut service = ProgramService::in_memory().expect("in-memory service");
        let envelope = envelope_with_actions(vec![AutonomyPlanAction::Inspect {
            request: AutonomyPlanInspectRequest {
                query: Some("search:return".to_string()),
                max_results: Some(0),
            },
            rationale: None,
        }]);

        let outcome = execute_plan(&mut service, &envelope);
        let action = &outcome.attempts[0].action_results[0];
        assert_eq!(action.status, AutonomyActionStatus::Failed);
        assert_eq!(
            action.error.as_ref().map(|err| err.code),
            Some(AutonomyExecutionErrorCode::InvalidActionPayload)
        );
    }

    #[test]
    fn history_without_operation_is_invalid_payload() {
        let mut service = ProgramService::in_memory().expect("in-memory service");
        let envelope = envelope_with_actions(vec![AutonomyPlanAction::History {
            request: AutonomyPlanHistoryRequest {
                operation: None,
                checkpoint: None,
                from_checkpoint: None,
                to_checkpoint: None,
                limit: None,
            },
            rationale: None,
        }]);

        let outcome = execute_plan(&mut service, &envelope);
        let action = &outcome.attempts[0].action_results[0];
        assert_eq!(action.status, AutonomyActionStatus::Failed);
        assert_eq!(
            action.error.as_ref().map(|err| err.code),
            Some(AutonomyExecutionErrorCode::InvalidActionPayload)
        );
    }
}

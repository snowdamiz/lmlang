//! Versioned planner contract schema for autonomous action planning.
//!
//! The contract defined here is the source of truth for structured planner
//! output consumed by server-side autonomous workflows.

use std::collections::HashMap;

use lmlang_core::id::FunctionId;
use serde::{Deserialize, Serialize};

use super::mutations::{Mutation, ProposeEditRequest};
use super::verify::VerifyScope;

/// Current planner contract version accepted by the server.
pub const AUTONOMY_PLAN_CONTRACT_V1: &str = "2026-02-19";

/// Default planner contract version used for backward compatibility.
pub fn default_plan_contract_version() -> String {
    AUTONOMY_PLAN_CONTRACT_V1.to_string()
}

/// Top-level planner envelope returned by the planner model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyPlanEnvelope {
    /// Contract version string. Example: `"2026-02-19"`.
    #[serde(default = "default_plan_contract_version")]
    pub version: String,
    /// Normalized user goal for this plan.
    pub goal: String,
    /// Optional planner metadata for auditability.
    #[serde(default)]
    pub metadata: AutonomyPlanMetadata,
    /// Ordered plan actions to execute deterministically.
    #[serde(default)]
    pub actions: Vec<AutonomyPlanAction>,
    /// Structured planner-side failure when no safe actions are proposed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<AutonomyPlanFailure>,
}

/// Planner metadata attached to an envelope.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyPlanMetadata {
    /// Planner implementation name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner: Option<String>,
    /// Model identifier used by planner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional free-form rationale for the produced plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Optional planner-side confidence [0.0, 1.0].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

/// Structured planner failure payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyPlanFailure {
    /// Machine-readable failure code.
    pub code: AutonomyPlanFailureCode,
    /// Human-readable failure message.
    pub message: String,
    /// Optional additional detail for logs or operators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Whether the prompt may succeed on retry.
    #[serde(default)]
    pub retryable: bool,
}

/// Enumerated planner failure codes for explicit handling.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyPlanFailureCode {
    ClarificationNeeded,
    UnsupportedGoal,
    UnsafePlan,
    PlannerUnavailable,
    ValidationFailed,
}

/// One ordered planner action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutonomyPlanAction {
    /// Apply graph mutations using existing mutation semantics.
    MutateBatch {
        request: AutonomyPlanMutationRequest,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rationale: Option<String>,
    },
    /// Run verify using existing verify scope semantics.
    Verify {
        request: AutonomyPlanVerifyRequest,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rationale: Option<String>,
    },
    /// Compile current program.
    Compile {
        request: AutonomyPlanCompileRequest,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rationale: Option<String>,
    },
    /// Simulate one function execution.
    Simulate {
        request: AutonomyPlanSimulateRequest,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rationale: Option<String>,
    },
    /// Perform inspect/query operation against program context.
    Inspect {
        request: AutonomyPlanInspectRequest,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rationale: Option<String>,
    },
    /// Perform history/checkpoint inspection operations.
    History {
        request: AutonomyPlanHistoryRequest,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rationale: Option<String>,
    },
}

/// Mutation-batch action payload.
///
/// Mirrors `ProposeEditRequest` semantics while keeping planner payload
/// independent from transport-layer request structs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyPlanMutationRequest {
    #[serde(default)]
    pub mutations: Vec<Mutation>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_hashes: Option<HashMap<u32, String>>,
}

impl AutonomyPlanMutationRequest {
    /// Convert planner mutation payload into `ProposeEditRequest`.
    pub fn to_propose_edit_request(&self) -> ProposeEditRequest {
        ProposeEditRequest {
            mutations: self.mutations.clone(),
            dry_run: self.dry_run,
            expected_hashes: self.expected_hashes.clone(),
        }
    }
}

/// Verify action payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyPlanVerifyRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<VerifyScope>,
}

/// Compile action payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyPlanCompileRequest {
    #[serde(default = "default_compile_opt_level")]
    pub opt_level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_triple: Option<String>,
    #[serde(default)]
    pub debug_symbols: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_function: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<String>,
}

fn default_compile_opt_level() -> String {
    "O0".to_string()
}

/// Simulate action payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyPlanSimulateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_id: Option<FunctionId>,
    #[serde(default)]
    pub inputs: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_enabled: Option<bool>,
}

/// Inspect/query action payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyPlanInspectRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<usize>,
}

/// Supported history operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyPlanHistoryOperation {
    ListEntries,
    ListCheckpoints,
    Undo,
    Redo,
    RestoreCheckpoint,
    Diff,
}

/// History action payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyPlanHistoryRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<AutonomyPlanHistoryOperation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_checkpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_checkpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Maximum number of actions allowed in one planner envelope.
pub const AUTONOMY_PLAN_MAX_ACTIONS: usize = 32;
const AUTONOMY_PLAN_MAX_MUTATIONS_PER_ACTION: usize = 128;
const AUTONOMY_PLAN_MAX_SIM_INPUTS: usize = 32;
const AUTONOMY_PLAN_MAX_INSPECT_RESULTS: usize = 200;

/// Structured semantic validation result for planner plans.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyPlanValidationResult {
    pub valid: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<AutonomyPlanValidationError>,
}

/// Machine-readable validation code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyPlanValidationCode {
    UnsupportedVersion,
    EmptyGoal,
    MissingActions,
    TooManyActions,
    MissingRequiredField,
    InvalidFieldValue,
    InvalidActionPayload,
    InvalidFailureShape,
}

/// One semantic validation error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AutonomyPlanValidationError {
    pub code: AutonomyPlanValidationCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl AutonomyPlanEnvelope {
    /// Run semantic validation against a parsed planner envelope.
    pub fn validate(&self) -> AutonomyPlanValidationResult {
        let mut errors = Vec::new();

        if self.version != AUTONOMY_PLAN_CONTRACT_V1 {
            push_validation_error(
                &mut errors,
                AutonomyPlanValidationCode::UnsupportedVersion,
                format!(
                    "unsupported planner contract version '{}' (expected '{}')",
                    self.version, AUTONOMY_PLAN_CONTRACT_V1
                ),
                None,
                Some("version".to_string()),
            );
        }

        if self.goal.trim().is_empty() {
            push_validation_error(
                &mut errors,
                AutonomyPlanValidationCode::EmptyGoal,
                "goal must not be empty".to_string(),
                None,
                Some("goal".to_string()),
            );
        }

        if self.actions.is_empty() && self.failure.is_none() {
            push_validation_error(
                &mut errors,
                AutonomyPlanValidationCode::MissingActions,
                "plan must include at least one action or a structured failure".to_string(),
                None,
                Some("actions".to_string()),
            );
        }

        if self.actions.len() > AUTONOMY_PLAN_MAX_ACTIONS {
            push_validation_error(
                &mut errors,
                AutonomyPlanValidationCode::TooManyActions,
                format!(
                    "plan has {} actions; limit is {}",
                    self.actions.len(),
                    AUTONOMY_PLAN_MAX_ACTIONS
                ),
                None,
                Some("actions".to_string()),
            );
        }

        if self.failure.is_some() && !self.actions.is_empty() {
            push_validation_error(
                &mut errors,
                AutonomyPlanValidationCode::InvalidFailureShape,
                "structured failure cannot be combined with executable actions".to_string(),
                None,
                Some("failure".to_string()),
            );
        }

        if let Some(failure) = &self.failure {
            if failure.message.trim().is_empty() {
                push_validation_error(
                    &mut errors,
                    AutonomyPlanValidationCode::MissingRequiredField,
                    "structured failure message must not be empty".to_string(),
                    None,
                    Some("failure.message".to_string()),
                );
            }
        }

        for (index, action) in self.actions.iter().enumerate() {
            validate_action(index, action, &mut errors);
        }

        AutonomyPlanValidationResult {
            valid: errors.is_empty(),
            errors,
        }
    }
}

fn validate_action(
    action_index: usize,
    action: &AutonomyPlanAction,
    errors: &mut Vec<AutonomyPlanValidationError>,
) {
    match action {
        AutonomyPlanAction::MutateBatch { request, .. } => {
            if request.mutations.is_empty() {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::MissingRequiredField,
                    "mutate_batch requires at least one mutation".to_string(),
                    Some(action_index),
                    Some("request.mutations".to_string()),
                );
            }
            if request.mutations.len() > AUTONOMY_PLAN_MAX_MUTATIONS_PER_ACTION {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::InvalidActionPayload,
                    format!(
                        "mutate_batch contains {} mutations; limit is {}",
                        request.mutations.len(),
                        AUTONOMY_PLAN_MAX_MUTATIONS_PER_ACTION
                    ),
                    Some(action_index),
                    Some("request.mutations".to_string()),
                );
            }
            if matches!(request.expected_hashes.as_ref(), Some(hashes) if hashes.is_empty()) {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::InvalidFieldValue,
                    "expected_hashes must not be an empty map when provided".to_string(),
                    Some(action_index),
                    Some("request.expected_hashes".to_string()),
                );
            }
        }
        AutonomyPlanAction::Verify { request, .. } => {
            if request.scope.is_none() {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::MissingRequiredField,
                    "verify action requires request.scope".to_string(),
                    Some(action_index),
                    Some("request.scope".to_string()),
                );
            }
        }
        AutonomyPlanAction::Compile { request, .. } => {
            if !matches!(request.opt_level.as_str(), "O0" | "O1" | "O2" | "O3") {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::InvalidFieldValue,
                    format!(
                        "compile opt_level '{}' is invalid (expected O0/O1/O2/O3)",
                        request.opt_level
                    ),
                    Some(action_index),
                    Some("request.opt_level".to_string()),
                );
            }
            if !option_is_present(&request.entry_function) {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::MissingRequiredField,
                    "compile action requires request.entry_function".to_string(),
                    Some(action_index),
                    Some("request.entry_function".to_string()),
                );
            }
        }
        AutonomyPlanAction::Simulate { request, .. } => {
            if request.function_id.is_none() {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::MissingRequiredField,
                    "simulate action requires request.function_id".to_string(),
                    Some(action_index),
                    Some("request.function_id".to_string()),
                );
            }
            if request.inputs.is_empty() {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::MissingRequiredField,
                    "simulate action requires one or more inputs".to_string(),
                    Some(action_index),
                    Some("request.inputs".to_string()),
                );
            }
            if request.inputs.len() > AUTONOMY_PLAN_MAX_SIM_INPUTS {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::InvalidActionPayload,
                    format!(
                        "simulate action includes {} inputs; limit is {}",
                        request.inputs.len(),
                        AUTONOMY_PLAN_MAX_SIM_INPUTS
                    ),
                    Some(action_index),
                    Some("request.inputs".to_string()),
                );
            }
        }
        AutonomyPlanAction::Inspect { request, .. } => {
            if !option_is_present(&request.query) {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::MissingRequiredField,
                    "inspect action requires request.query".to_string(),
                    Some(action_index),
                    Some("request.query".to_string()),
                );
            }
            if matches!(request.max_results, Some(0)) {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::InvalidFieldValue,
                    "inspect max_results must be > 0".to_string(),
                    Some(action_index),
                    Some("request.max_results".to_string()),
                );
            }
            if let Some(max_results) = request.max_results {
                if max_results > AUTONOMY_PLAN_MAX_INSPECT_RESULTS {
                    push_validation_error(
                        errors,
                        AutonomyPlanValidationCode::InvalidActionPayload,
                        format!(
                            "inspect max_results {} exceeds limit {}",
                            max_results, AUTONOMY_PLAN_MAX_INSPECT_RESULTS
                        ),
                        Some(action_index),
                        Some("request.max_results".to_string()),
                    );
                }
            }
        }
        AutonomyPlanAction::History { request, .. } => {
            let Some(operation) = request.operation.as_ref() else {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::MissingRequiredField,
                    "history action requires request.operation".to_string(),
                    Some(action_index),
                    Some("request.operation".to_string()),
                );
                return;
            };

            if matches!(request.limit, Some(0)) {
                push_validation_error(
                    errors,
                    AutonomyPlanValidationCode::InvalidFieldValue,
                    "history limit must be > 0 when provided".to_string(),
                    Some(action_index),
                    Some("request.limit".to_string()),
                );
            }

            match operation {
                AutonomyPlanHistoryOperation::RestoreCheckpoint => {
                    if !option_is_present(&request.checkpoint) {
                        push_validation_error(
                            errors,
                            AutonomyPlanValidationCode::MissingRequiredField,
                            "history restore_checkpoint requires request.checkpoint".to_string(),
                            Some(action_index),
                            Some("request.checkpoint".to_string()),
                        );
                    }
                }
                AutonomyPlanHistoryOperation::Diff => {
                    if request.from_checkpoint.is_none() && request.to_checkpoint.is_none() {
                        push_validation_error(
                            errors,
                            AutonomyPlanValidationCode::MissingRequiredField,
                            "history diff requires from_checkpoint and/or to_checkpoint"
                                .to_string(),
                            Some(action_index),
                            Some("request.from_checkpoint".to_string()),
                        );
                    }
                }
                AutonomyPlanHistoryOperation::ListEntries
                | AutonomyPlanHistoryOperation::ListCheckpoints
                | AutonomyPlanHistoryOperation::Undo
                | AutonomyPlanHistoryOperation::Redo => {}
            }
        }
    }
}

fn option_is_present(value: &Option<String>) -> bool {
    matches!(value.as_ref(), Some(text) if !text.trim().is_empty())
}

fn push_validation_error(
    errors: &mut Vec<AutonomyPlanValidationError>,
    code: AutonomyPlanValidationCode,
    message: String,
    action_index: Option<usize>,
    field: Option<String>,
) {
    errors.push(AutonomyPlanValidationError {
        code,
        message,
        action_index,
        field,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::id::NodeId;
    use lmlang_core::ops::{ComputeNodeOp, ComputeOp};
    use lmlang_core::type_id::TypeId;
    use lmlang_core::types::Visibility;

    fn valid_minimal_plan() -> AutonomyPlanEnvelope {
        AutonomyPlanEnvelope {
            version: AUTONOMY_PLAN_CONTRACT_V1.to_string(),
            goal: "build calculator".to_string(),
            metadata: AutonomyPlanMetadata::default(),
            actions: vec![
                AutonomyPlanAction::MutateBatch {
                    request: AutonomyPlanMutationRequest {
                        mutations: vec![Mutation::AddFunction {
                            name: "calc".to_string(),
                            module: lmlang_core::id::ModuleId(0),
                            params: vec![("x".to_string(), TypeId::I32)],
                            return_type: TypeId::I32,
                            visibility: Visibility::Public,
                        }],
                        dry_run: false,
                        expected_hashes: None,
                    },
                    rationale: None,
                },
                AutonomyPlanAction::Verify {
                    request: AutonomyPlanVerifyRequest {
                        scope: Some(VerifyScope::Local),
                    },
                    rationale: None,
                },
                AutonomyPlanAction::Compile {
                    request: AutonomyPlanCompileRequest {
                        opt_level: "O1".to_string(),
                        target_triple: None,
                        debug_symbols: false,
                        entry_function: Some("calc".to_string()),
                        output_dir: None,
                    },
                    rationale: None,
                },
                AutonomyPlanAction::Simulate {
                    request: AutonomyPlanSimulateRequest {
                        function_id: Some(lmlang_core::id::FunctionId(1)),
                        inputs: vec![serde_json::json!(7)],
                        trace_enabled: Some(false),
                    },
                    rationale: None,
                },
                AutonomyPlanAction::Inspect {
                    request: AutonomyPlanInspectRequest {
                        query: Some("calc function".to_string()),
                        max_results: Some(5),
                    },
                    rationale: None,
                },
                AutonomyPlanAction::History {
                    request: AutonomyPlanHistoryRequest {
                        operation: Some(AutonomyPlanHistoryOperation::ListEntries),
                        checkpoint: None,
                        from_checkpoint: None,
                        to_checkpoint: None,
                        limit: Some(10),
                    },
                    rationale: None,
                },
            ],
            failure: None,
        }
    }

    #[test]
    fn autonomy_plan_round_trips_multi_step_actions() {
        let plan = AutonomyPlanEnvelope {
            version: AUTONOMY_PLAN_CONTRACT_V1.to_string(),
            goal: "create a simple calculator".to_string(),
            metadata: AutonomyPlanMetadata {
                planner: Some("phase14-planner".to_string()),
                model: Some("test-model".to_string()),
                notes: Some("plan includes mutation and verification".to_string()),
                confidence: Some(0.82),
            },
            actions: vec![
                AutonomyPlanAction::MutateBatch {
                    request: AutonomyPlanMutationRequest {
                        mutations: vec![
                            Mutation::AddFunction {
                                name: "add".to_string(),
                                module: lmlang_core::id::ModuleId(0),
                                params: vec![
                                    ("left".to_string(), TypeId::I32),
                                    ("right".to_string(), TypeId::I32),
                                ],
                                return_type: TypeId::I32,
                                visibility: Visibility::Public,
                            },
                            Mutation::InsertNode {
                                op: ComputeNodeOp::Core(ComputeOp::Parameter { index: 0 }),
                                owner: lmlang_core::id::FunctionId(1),
                            },
                        ],
                        dry_run: false,
                        expected_hashes: None,
                    },
                    rationale: Some("create calculator entrypoints".to_string()),
                },
                AutonomyPlanAction::Verify {
                    request: AutonomyPlanVerifyRequest {
                        scope: Some(VerifyScope::Full),
                    },
                    rationale: Some("verify post-mutation graph integrity".to_string()),
                },
                AutonomyPlanAction::Compile {
                    request: AutonomyPlanCompileRequest {
                        opt_level: "O1".to_string(),
                        target_triple: None,
                        debug_symbols: false,
                        entry_function: Some("add".to_string()),
                        output_dir: None,
                    },
                    rationale: None,
                },
                AutonomyPlanAction::Inspect {
                    request: AutonomyPlanInspectRequest {
                        query: Some("calculator".to_string()),
                        max_results: Some(10),
                    },
                    rationale: None,
                },
                AutonomyPlanAction::History {
                    request: AutonomyPlanHistoryRequest {
                        operation: Some(AutonomyPlanHistoryOperation::ListEntries),
                        checkpoint: None,
                        from_checkpoint: None,
                        to_checkpoint: None,
                        limit: Some(20),
                    },
                    rationale: None,
                },
                AutonomyPlanAction::Simulate {
                    request: AutonomyPlanSimulateRequest {
                        function_id: Some(lmlang_core::id::FunctionId(1)),
                        inputs: vec![serde_json::json!(1), serde_json::json!(2)],
                        trace_enabled: Some(true),
                    },
                    rationale: Some("confirm runtime behavior".to_string()),
                },
            ],
            failure: None,
        };

        let encoded = serde_json::to_value(&plan).expect("plan should serialize");
        let decoded: AutonomyPlanEnvelope =
            serde_json::from_value(encoded.clone()).expect("plan should deserialize");

        assert_eq!(decoded.version, AUTONOMY_PLAN_CONTRACT_V1);
        assert_eq!(decoded.goal, "create a simple calculator");
        assert_eq!(decoded.actions.len(), 6);
        assert_eq!(encoded["actions"][0]["type"], "mutate_batch");
        assert_eq!(encoded["actions"][1]["type"], "verify");
        assert_eq!(encoded["actions"][5]["type"], "simulate");
    }

    #[test]
    fn autonomy_plan_uses_predictable_defaults() {
        let decoded: AutonomyPlanEnvelope = serde_json::from_value(serde_json::json!({
            "goal": "bootstrap hello world"
        }))
        .expect("plan should deserialize with defaults");

        assert_eq!(decoded.version, AUTONOMY_PLAN_CONTRACT_V1);
        assert!(decoded.actions.is_empty());
        assert!(decoded.failure.is_none());
        assert!(decoded.metadata.planner.is_none());
    }

    #[test]
    fn mutation_payload_converts_to_propose_edit_request() {
        let payload = AutonomyPlanMutationRequest {
            mutations: vec![Mutation::RemoveNode { node_id: NodeId(9) }],
            dry_run: true,
            expected_hashes: Some(HashMap::from([(1u32, "abc123".to_string())])),
        };

        let request = payload.to_propose_edit_request();
        assert_eq!(request.mutations.len(), 1);
        assert!(request.dry_run);
        assert_eq!(
            request
                .expected_hashes
                .as_ref()
                .and_then(|m| m.get(&1))
                .map(String::as_str),
            Some("abc123")
        );
    }

    #[test]
    fn structured_failure_serializes_with_machine_code() {
        let envelope = AutonomyPlanEnvelope {
            version: AUTONOMY_PLAN_CONTRACT_V1.to_string(),
            goal: "unfulfillable goal".to_string(),
            metadata: AutonomyPlanMetadata::default(),
            actions: Vec::new(),
            failure: Some(AutonomyPlanFailure {
                code: AutonomyPlanFailureCode::UnsupportedGoal,
                message: "goal requires unavailable runtime".to_string(),
                detail: Some("missing capability: ffi runtime".to_string()),
                retryable: false,
            }),
        };

        let encoded = serde_json::to_value(&envelope).expect("failure should serialize");
        assert_eq!(encoded["failure"]["code"], "unsupported_goal");
        assert_eq!(
            encoded["failure"]["message"],
            "goal requires unavailable runtime"
        );
    }

    #[test]
    fn validation_accepts_multi_step_plan() {
        let result = valid_minimal_plan().validate();
        assert!(
            result.valid,
            "expected valid plan, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn validation_rejects_unsupported_version() {
        let mut plan = valid_minimal_plan();
        plan.version = "v999".to_string();

        let result = plan.validate();
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| e.code == AutonomyPlanValidationCode::UnsupportedVersion));
    }

    #[test]
    fn validation_rejects_empty_action_plan_without_failure() {
        let mut plan = valid_minimal_plan();
        plan.actions.clear();

        let result = plan.validate();
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| e.code == AutonomyPlanValidationCode::MissingActions));
    }

    #[test]
    fn validation_rejects_missing_required_action_fields() {
        let mut plan = valid_minimal_plan();
        plan.actions[1] = AutonomyPlanAction::Verify {
            request: AutonomyPlanVerifyRequest { scope: None },
            rationale: None,
        };
        plan.actions[2] = AutonomyPlanAction::Compile {
            request: AutonomyPlanCompileRequest {
                opt_level: "O1".to_string(),
                target_triple: None,
                debug_symbols: false,
                entry_function: None,
                output_dir: None,
            },
            rationale: None,
        };

        let result = plan.validate();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| {
            e.code == AutonomyPlanValidationCode::MissingRequiredField
                && e.field.as_deref() == Some("request.scope")
        }));
        assert!(result.errors.iter().any(|e| {
            e.code == AutonomyPlanValidationCode::MissingRequiredField
                && e.field.as_deref() == Some("request.entry_function")
        }));
    }

    #[test]
    fn validation_rejects_invalid_action_payload_values() {
        let mut plan = valid_minimal_plan();
        plan.actions[2] = AutonomyPlanAction::Compile {
            request: AutonomyPlanCompileRequest {
                opt_level: "O9".to_string(),
                target_triple: None,
                debug_symbols: false,
                entry_function: Some("calc".to_string()),
                output_dir: None,
            },
            rationale: None,
        };
        plan.actions[4] = AutonomyPlanAction::Inspect {
            request: AutonomyPlanInspectRequest {
                query: Some("calc".to_string()),
                max_results: Some(0),
            },
            rationale: None,
        };

        let result = plan.validate();
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| {
            e.code == AutonomyPlanValidationCode::InvalidFieldValue
                && e.field.as_deref() == Some("request.opt_level")
        }));
        assert!(result.errors.iter().any(|e| {
            e.code == AutonomyPlanValidationCode::InvalidFieldValue
                && e.field.as_deref() == Some("request.max_results")
        }));
    }

    #[test]
    fn validation_requires_failure_only_when_no_actions_present() {
        let mut plan = valid_minimal_plan();
        plan.failure = Some(AutonomyPlanFailure {
            code: AutonomyPlanFailureCode::ValidationFailed,
            message: "planner refused unsafe sequence".to_string(),
            detail: None,
            retryable: false,
        });

        let result = plan.validate();
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| e.code == AutonomyPlanValidationCode::InvalidFailureShape));
    }
}

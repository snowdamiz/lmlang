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

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::id::NodeId;
    use lmlang_core::ops::{ComputeNodeOp, ComputeOp};
    use lmlang_core::type_id::TypeId;
    use lmlang_core::types::Visibility;

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
}

//! Planner runtime adapter for structured autonomous planning.
//!
//! This module converts natural-language goals into versioned planner plans,
//! validates the contract, and returns typed success/failure outcomes that
//! handlers can expose directly.

use crate::concurrency::AgentLlmConfig;
use crate::llm_provider::run_external_chat_json;
use crate::schema::autonomy_plan::{
    AutonomyPlanAction, AutonomyPlanEnvelope, AutonomyPlanFailure, AutonomyPlanFailureCode,
    AutonomyPlanValidationError, AUTONOMY_PLAN_CONTRACT_V1,
};

/// Structured planner result consumed by handlers and autonomous runner.
#[derive(Debug, Clone)]
pub enum PlannerOutcome {
    Accepted(PlannerAccepted),
    Rejected(PlannerRejected),
}

/// Successful planner result with validated executable actions.
#[derive(Debug, Clone)]
pub struct PlannerAccepted {
    pub version: String,
    pub goal: String,
    pub action_count: usize,
    pub actions: Vec<PlannerActionSummary>,
    pub envelope: AutonomyPlanEnvelope,
}

/// Structured planner rejection with stable machine-readable code.
#[derive(Debug, Clone)]
pub struct PlannerRejected {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub version: Option<String>,
    pub validation_errors: Vec<AutonomyPlanValidationError>,
}

/// Lightweight action summary projected for API responses/transcripts.
#[derive(Debug, Clone)]
pub struct PlannerActionSummary {
    pub kind: String,
    pub summary: String,
}

impl PlannerOutcome {
    pub fn status(&self) -> &'static str {
        match self {
            Self::Accepted(_) => "accepted",
            Self::Rejected(_) => "failed",
        }
    }
}

/// Build and validate a planner outcome for a prompt.
///
/// This function never returns transport-layer errors to callers. Provider and
/// parse failures are normalized into structured `PlannerOutcome::Rejected`.
pub async fn plan_for_prompt(
    llm: &AgentLlmConfig,
    prompt: &str,
    transcript: &[String],
) -> PlannerOutcome {
    if !llm.is_configured() {
        return planner_rejected(
            "planner_unconfigured",
            "Planner provider is not configured for this agent.",
            false,
            None,
            Vec::new(),
        );
    }

    let planner_prompt = build_planner_prompt(prompt, transcript);
    let raw = match run_external_chat_json(llm, &planner_prompt).await {
        Ok(raw) => raw,
        Err(err) => {
            return planner_rejected(
                "planner_provider_error",
                &format!("Planner provider request failed: {}", err),
                true,
                None,
                Vec::new(),
            );
        }
    };

    evaluate_planner_json_text(&raw)
}

/// Evaluate planner JSON text and return a structured planner outcome.
pub fn evaluate_planner_json_text(raw: &str) -> PlannerOutcome {
    let value: serde_json::Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(err) => {
            return planner_rejected(
                "planner_invalid_json",
                &format!("Planner response was not valid JSON: {}", err),
                true,
                None,
                Vec::new(),
            );
        }
    };

    evaluate_planner_value(value)
}

/// Evaluate parsed planner JSON payload and return a structured planner outcome.
pub fn evaluate_planner_value(value: serde_json::Value) -> PlannerOutcome {
    let envelope: AutonomyPlanEnvelope = match serde_json::from_value(value) {
        Ok(envelope) => envelope,
        Err(err) => {
            return planner_rejected(
                "planner_contract_parse_failed",
                &format!("Planner JSON did not match contract schema: {}", err),
                true,
                None,
                Vec::new(),
            );
        }
    };

    let validation = envelope.validate();
    if !validation.valid {
        return planner_rejected(
            "planner_validation_failed",
            "Planner output failed semantic validation.",
            false,
            Some(envelope.version),
            validation.errors,
        );
    }

    if let Some(failure) = envelope.failure.as_ref() {
        return planner_rejected_from_failure(failure, Some(envelope.version));
    }

    let actions = summarize_actions(&envelope.actions);
    PlannerOutcome::Accepted(PlannerAccepted {
        version: envelope.version.clone(),
        goal: envelope.goal.clone(),
        action_count: envelope.actions.len(),
        actions,
        envelope,
    })
}

fn build_planner_prompt(goal: &str, transcript: &[String]) -> String {
    let transcript_block = if transcript.is_empty() {
        "No recent transcript.".to_string()
    } else {
        transcript.join("\n")
    };

    format!(
        "You are the lmlang autonomous planner.\n\
Return only JSON with no markdown and no surrounding text.\n\
Use planner contract version '{}'.\n\
Allowed action types: mutate_batch, verify, compile, simulate, inspect, history.\n\
Rules:\n\
- Use an ordered actions array for executable plans.\n\
- If no safe plan exists, return a structured failure object and empty actions.\n\
- Keep payloads minimal and deterministic.\n\
\n\
Goal:\n{}\n\
\n\
Recent transcript:\n{}",
        AUTONOMY_PLAN_CONTRACT_V1, goal, transcript_block
    )
}

fn summarize_actions(actions: &[AutonomyPlanAction]) -> Vec<PlannerActionSummary> {
    actions
        .iter()
        .map(|action| match action {
            AutonomyPlanAction::MutateBatch { request, .. } => PlannerActionSummary {
                kind: "mutate_batch".to_string(),
                summary: format!(
                    "{} mutation(s), dry_run={}",
                    request.mutations.len(),
                    request.dry_run
                ),
            },
            AutonomyPlanAction::Verify { request, .. } => PlannerActionSummary {
                kind: "verify".to_string(),
                summary: format!("scope={:?}", request.scope),
            },
            AutonomyPlanAction::Compile { request, .. } => PlannerActionSummary {
                kind: "compile".to_string(),
                summary: format!(
                    "entry_function={}, opt_level={}",
                    request.entry_function.as_deref().unwrap_or("<none>"),
                    request.opt_level
                ),
            },
            AutonomyPlanAction::Simulate { request, .. } => PlannerActionSummary {
                kind: "simulate".to_string(),
                summary: format!(
                    "function_id={}, inputs={}",
                    request
                        .function_id
                        .map(|id| id.0.to_string())
                        .unwrap_or_else(|| "<none>".to_string()),
                    request.inputs.len()
                ),
            },
            AutonomyPlanAction::Inspect { request, .. } => PlannerActionSummary {
                kind: "inspect".to_string(),
                summary: format!(
                    "query='{}', max_results={}",
                    request.query.as_deref().unwrap_or("<none>"),
                    request
                        .max_results
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "<default>".to_string())
                ),
            },
            AutonomyPlanAction::History { request, .. } => PlannerActionSummary {
                kind: "history".to_string(),
                summary: format!("operation={:?}", request.operation),
            },
        })
        .collect()
}

fn planner_rejected_from_failure(
    failure: &AutonomyPlanFailure,
    version: Option<String>,
) -> PlannerOutcome {
    let code = match failure.code {
        AutonomyPlanFailureCode::ClarificationNeeded => "clarification_needed",
        AutonomyPlanFailureCode::UnsupportedGoal => "unsupported_goal",
        AutonomyPlanFailureCode::UnsafePlan => "unsafe_plan",
        AutonomyPlanFailureCode::PlannerUnavailable => "planner_unavailable",
        AutonomyPlanFailureCode::ValidationFailed => "validation_failed",
    };

    planner_rejected(code, &failure.message, failure.retryable, version, Vec::new())
}

fn planner_rejected(
    code: &str,
    message: &str,
    retryable: bool,
    version: Option<String>,
    validation_errors: Vec<AutonomyPlanValidationError>,
) -> PlannerOutcome {
    PlannerOutcome::Rejected(PlannerRejected {
        code: code.to_string(),
        message: message.to_string(),
        retryable,
        version,
        validation_errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_planner_json_text_accepts_valid_multi_step_plan() {
        let outcome = evaluate_planner_json_text(
            r#"{
                "version": "2026-02-19",
                "goal": "build calculator",
                "actions": [
                    {
                        "type": "mutate_batch",
                        "request": {
                            "mutations": [{
                                "type": "AddFunction",
                                "name": "calc",
                                "module": 0,
                                "params": [],
                                "return_type": 3,
                                "visibility": "Public"
                            }],
                            "dry_run": false
                        }
                    },
                    {
                        "type": "verify",
                        "request": { "scope": "Full" }
                    },
                    {
                        "type": "compile",
                        "request": {
                            "entry_function": "calc",
                            "opt_level": "O1"
                        }
                    }
                ]
            }"#,
        );

        match outcome {
            PlannerOutcome::Accepted(accepted) => {
                assert_eq!(accepted.version, AUTONOMY_PLAN_CONTRACT_V1);
                assert_eq!(accepted.action_count, 3);
                assert_eq!(accepted.actions[0].kind, "mutate_batch");
            }
            PlannerOutcome::Rejected(rejected) => {
                panic!("expected accepted planner outcome, got: {:?}", rejected)
            }
        }
    }

    #[test]
    fn evaluate_planner_json_text_rejects_invalid_json() {
        let outcome = evaluate_planner_json_text("not json");
        match outcome {
            PlannerOutcome::Rejected(rejected) => {
                assert_eq!(rejected.code, "planner_invalid_json");
            }
            PlannerOutcome::Accepted(_) => panic!("expected rejection for invalid json"),
        }
    }

    #[test]
    fn evaluate_planner_json_text_rejects_unsupported_version() {
        let outcome = evaluate_planner_json_text(
            r#"{
                "version": "2025-01-01",
                "goal": "build calculator",
                "actions": [
                    {
                        "type": "verify",
                        "request": { "scope": "Full" }
                    }
                ]
            }"#,
        );

        match outcome {
            PlannerOutcome::Rejected(rejected) => {
                assert_eq!(rejected.code, "planner_validation_failed");
                assert!(rejected
                    .validation_errors
                    .iter()
                    .any(|e| matches!(e.code, crate::schema::autonomy_plan::AutonomyPlanValidationCode::UnsupportedVersion)));
            }
            PlannerOutcome::Accepted(_) => panic!("expected validation rejection"),
        }
    }

    #[test]
    fn evaluate_planner_json_text_rejects_semantic_validation_errors() {
        let outcome = evaluate_planner_json_text(
            r#"{
                "version": "2026-02-19",
                "goal": "build calculator",
                "actions": [
                    {
                        "type": "compile",
                        "request": {
                            "opt_level": "O9"
                        }
                    }
                ]
            }"#,
        );

        match outcome {
            PlannerOutcome::Rejected(rejected) => {
                assert_eq!(rejected.code, "planner_validation_failed");
                assert!(rejected.validation_errors.iter().any(
                    |e| e.field.as_deref() == Some("request.entry_function")
                ));
                assert!(rejected
                    .validation_errors
                    .iter()
                    .any(|e| e.field.as_deref() == Some("request.opt_level")));
            }
            PlannerOutcome::Accepted(_) => panic!("expected semantic rejection"),
        }
    }
}

//! Structured execution evidence and stop-reason schema for autonomous runs.
//!
//! These types capture machine-readable outcomes for planner action execution,
//! including per-action status/error details and deterministic terminal reasons.

use serde::{Deserialize, Serialize};

/// High-level status for one autonomous execution outcome payload.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyExecutionStatus {
    Succeeded,
    Failed,
}

/// Status for one dispatched planner action.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyActionStatus {
    Succeeded,
    Failed,
    Skipped,
}

/// Machine-readable action error classification.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyExecutionErrorCode {
    InvalidActionPayload,
    UnsupportedAction,
    NotFound,
    BadRequest,
    ValidationFailed,
    Conflict,
    InternalError,
}

/// Structured action failure details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AutonomyExecutionError {
    pub code: AutonomyExecutionErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<AutonomyDiagnostics>,
}

impl AutonomyExecutionError {
    pub fn new(
        code: AutonomyExecutionErrorCode,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            details: None,
            diagnostics: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn with_diagnostics(mut self, diagnostics: AutonomyDiagnostics) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }
}

/// Canonical diagnostic class emitted for autonomous repair feedback.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyDiagnosticsClass {
    VerifyFailure,
    CompileFailure,
    ActionFailure,
}

/// Compact, machine-readable diagnostics metadata for targeted repair.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AutonomyDiagnostics {
    pub class: AutonomyDiagnosticsClass,
    pub retryable: bool,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

impl AutonomyDiagnostics {
    pub fn new(
        class: AutonomyDiagnosticsClass,
        retryable: bool,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            class,
            retryable,
            summary: summary.into(),
            messages: Vec::new(),
            detail: None,
        }
    }

    pub fn with_messages(mut self, messages: Vec<String>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

/// Typed result for one planner action execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AutonomyActionExecutionResult {
    pub action_index: usize,
    pub kind: String,
    pub status: AutonomyActionStatus,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AutonomyExecutionError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<AutonomyDiagnostics>,
}

impl AutonomyActionExecutionResult {
    pub fn succeeded(
        action_index: usize,
        kind: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            action_index,
            kind: kind.into(),
            status: AutonomyActionStatus::Succeeded,
            summary: summary.into(),
            error: None,
            detail: None,
            diagnostics: None,
        }
    }

    pub fn failed(
        action_index: usize,
        kind: impl Into<String>,
        summary: impl Into<String>,
        error: AutonomyExecutionError,
    ) -> Self {
        Self {
            action_index,
            kind: kind.into(),
            status: AutonomyActionStatus::Failed,
            summary: summary.into(),
            error: Some(error),
            detail: None,
            diagnostics: None,
        }
    }

    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }

    pub fn with_diagnostics(mut self, diagnostics: AutonomyDiagnostics) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }
}

/// Stable terminal reason codes for autonomous run stops.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReasonCode {
    Completed,
    PlannerRejectedNonRetryable,
    PlannerRejectedRetryBudgetExhausted,
    ActionFailedRetryable,
    ActionFailedNonRetryable,
    VerifyFailed,
    RetryBudgetExhausted,
    OperatorStopped,
    RunnerInternalError,
}

/// Structured terminal stop reason payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StopReason {
    pub code: StopReasonCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

impl StopReason {
    pub fn new(code: StopReasonCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

/// One autonomous attempt summary (for bounded retry loops).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AutonomyExecutionAttemptSummary {
    pub attempt: u32,
    pub max_attempts: u32,
    pub planner_status: String,
    pub action_count: usize,
    pub succeeded_actions: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub action_results: Vec<AutonomyActionExecutionResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
}

impl AutonomyExecutionAttemptSummary {
    pub fn with_stop_reason(mut self, reason: StopReason) -> Self {
        self.stop_reason = Some(reason);
        self
    }

    /// Latest diagnostics for this attempt from action or nested error payloads.
    pub fn latest_diagnostics(&self) -> Option<&AutonomyDiagnostics> {
        self.action_results.iter().rev().find_map(|action| {
            action.diagnostics.as_ref().or_else(|| {
                action
                    .error
                    .as_ref()
                    .and_then(|error| error.diagnostics.as_ref())
            })
        })
    }

    /// Attempt stop reason, falling back to a session-level terminal reason.
    pub fn terminal_stop_reason<'a>(
        &'a self,
        fallback: Option<&'a StopReason>,
    ) -> Option<&'a StopReason> {
        self.stop_reason.as_ref().or(fallback)
    }
}

/// Top-level execution payload stored on sessions and surfaced via APIs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AutonomyExecutionOutcome {
    pub goal: String,
    pub version: String,
    pub status: AutonomyExecutionStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<AutonomyExecutionAttemptSummary>,
    pub stop_reason: StopReason,
}

impl AutonomyExecutionOutcome {
    pub fn from_single_attempt(
        goal: impl Into<String>,
        version: impl Into<String>,
        status: AutonomyExecutionStatus,
        attempt: AutonomyExecutionAttemptSummary,
        stop_reason: StopReason,
    ) -> Self {
        Self {
            goal: goal.into(),
            version: version.into(),
            status,
            attempts: vec![attempt],
            stop_reason,
        }
    }
}

/// Return the most recent bounded slice of attempts for timeline rendering.
pub fn bounded_attempt_history(
    attempts: &[AutonomyExecutionAttemptSummary],
    max_entries: usize,
) -> &[AutonomyExecutionAttemptSummary] {
    let start = attempts.len().saturating_sub(max_entries);
    &attempts[start..]
}

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::context::WorkflowContext;
use crate::error::{FlowError, Result};
use crate::model::{FlowEventEnvelope, JsonValue, WorkflowSpec};

/// Workflow replay request passed to a runtime implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WorkflowInvocation {
    /// Durable identifier of the workflow run being replayed.
    pub run_id: String,
    /// Workflow definition recorded when the run was created.
    pub spec: WorkflowSpec,
    /// Original workflow input.
    pub input: JsonValue,
    /// Complete persisted event history in sequence order.
    pub history: Vec<FlowEventEnvelope>,
}

impl WorkflowInvocation {
    /// Create a workflow invocation from its complete durable replay input.
    pub fn new(
        run_id: impl Into<String>,
        spec: WorkflowSpec,
        input: JsonValue,
        history: Vec<FlowEventEnvelope>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            spec,
            input,
            history,
        }
    }

    /// Build a deterministic helper view over this workflow invocation.
    pub fn context(&self) -> WorkflowContext<'_> {
        WorkflowContext::new(self)
    }

    /// Decode the workflow input into a host-defined serde type.
    pub fn input_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.input.clone()).map_err(FlowError::from)
    }
}

/// Step execution request passed to a runtime implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StepInvocation {
    /// Durable identifier of the workflow run that scheduled the step.
    pub run_id: String,
    /// Replay-stable identifier of this step invocation.
    pub step_id: String,
    /// One-based attempt number being executed. A redelivery after an
    /// ambiguous host boundary keeps the same attempt number.
    #[serde(default)]
    pub attempt: u32,
    /// Host-defined step handler name.
    pub step_name: String,
    /// Input supplied by the workflow command.
    pub input: JsonValue,
    /// Complete persisted workflow history in sequence order.
    pub history: Vec<FlowEventEnvelope>,
    /// Opaque, stable key for the external side effect of this attempt.
    ///
    /// The key is derived only from the run, step, and attempt identities, so
    /// retries and crash redelivery can safely use it for host-side
    /// idempotency and reconciliation.
    #[serde(default)]
    pub idempotency_key: String,
}

/// First-class activity execution request passed to a runtime implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ActivityInvocation {
    /// Durable identifier of the workflow run that scheduled the activity.
    pub run_id: String,
    /// Replay-stable identifier of this activity.
    pub activity_id: String,
    /// One-based attempt number being executed.
    pub attempt: u32,
    /// Stable identity for this attempt.
    #[serde(default)]
    pub attempt_id: String,
    /// Host-defined activity handler name.
    pub activity_name: String,
    /// Input supplied by the workflow command.
    pub input: JsonValue,
    /// Complete persisted workflow history in sequence order.
    pub history: Vec<FlowEventEnvelope>,
    /// Opaque, stable key for the external side effect of this attempt.
    #[serde(default)]
    pub idempotency_key: String,
    /// Fencing token assigned to this attempt.
    #[serde(default)]
    pub fencing_token: String,
    /// Persisted deadline of this attempt, retained across redelivery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<chrono::DateTime<chrono::Utc>>,
}

impl ActivityInvocation {
    /// Decode the activity input into a host-defined serde type.
    pub fn input_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.input.clone()).map_err(FlowError::from)
    }
}

impl StepInvocation {
    /// Create a step invocation from its complete durable execution input.
    pub fn new(
        run_id: impl Into<String>,
        step_id: impl Into<String>,
        step_name: impl Into<String>,
        input: JsonValue,
        history: Vec<FlowEventEnvelope>,
    ) -> Self {
        let run_id = run_id.into();
        let step_id = step_id.into();
        let attempt = history
            .iter()
            .rev()
            .find_map(|envelope| match &envelope.event {
                crate::model::FlowEvent::StepStarted {
                    step_id: started_step_id,
                    attempt,
                } if started_step_id == &step_id => Some(*attempt),
                _ => None,
            })
            .unwrap_or(0);
        Self {
            run_id: run_id.clone(),
            step_id: step_id.clone(),
            attempt,
            step_name: step_name.into(),
            input,
            history,
            idempotency_key: step_attempt_idempotency_key(&run_id, &step_id, attempt),
        }
    }

    /// Decode the step input into a host-defined serde type.
    pub fn input_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.input.clone()).map_err(FlowError::from)
    }
}

pub(crate) fn step_attempt_idempotency_key(run_id: &str, step_id: &str, attempt: u32) -> String {
    // Length prefixes keep the key unambiguous even when host-defined IDs
    // contain separators. Callers should treat the result as opaque.
    format!(
        "flow.step.v1/{}/{}{}:{}/{}",
        run_id.len(),
        run_id,
        step_id.len(),
        step_id,
        attempt
    )
}

pub(crate) fn activity_attempt_id(run_id: &str, activity_id: &str, attempt: u32) -> String {
    format!(
        "flow.activity.attempt.v1/{}/{}{}:{}/{}",
        run_id.len(),
        run_id,
        activity_id.len(),
        activity_id,
        attempt
    )
}

pub(crate) fn activity_attempt_idempotency_key(
    run_id: &str,
    activity_id: &str,
    attempt: u32,
) -> String {
    format!(
        "flow.activity.idempotency.v1/{}/{}{}:{}/{}",
        run_id.len(),
        run_id,
        activity_id.len(),
        activity_id,
        attempt
    )
}

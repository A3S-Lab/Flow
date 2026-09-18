use std::sync::Arc;

use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, FlowRuntime, FlowTask,
    FlowWorker, InMemoryEventStore, RuntimeCommand, SignalWaitStatus, StepInvocation,
    WorkflowInvocation, WorkflowRunStatus, WorkflowSignal, WorkflowSpec, WorkflowUpdate,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};

const APPROVAL_SIGNAL: &str = "order.approved";
const RELEASE_SIGNAL: &str = "order.released";

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("test.signals", "1", "tests::signals", "main")
        .with_signal(APPROVAL_SIGNAL)
        .with_signal(RELEASE_SIGNAL)
}

#[derive(Debug, Deserialize)]
struct Approval {
    approved: bool,
}

struct ApprovalRuntime;

#[async_trait]
impl FlowRuntime for ApprovalRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        let Some(approval) = context.signal_payload_as::<Approval>("approval")? else {
            return Ok(context.wait_for_signal("approval", APPROVAL_SIGNAL));
        };
        Ok(context.complete(json!({ "approved": approval.approved })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime(
            "signal workflow does not execute steps".to_string(),
        ))
    }
}

#[tokio::test]
async fn named_signal_resumes_a_durable_wait_and_exposes_typed_payload() {
    let engine = FlowEngine::in_memory(Arc::new(ApprovalRuntime));
    let run_id = engine
        .start_with_id("signal-approval", spec(), json!({}))
        .await
        .unwrap();

    let waiting = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(
        waiting.signal_waits["approval"].status,
        SignalWaitStatus::Waiting
    );
    assert_eq!(
        waiting.signal_waits["approval"].signal_name,
        APPROVAL_SIGNAL
    );
    assert_eq!(engine.run_summary().await.unwrap().open_signal_waits, 1);

    let completed = engine
        .send_signal(
            &run_id,
            WorkflowSignal::new(
                "approval-delivery-1",
                APPROVAL_SIGNAL,
                json!({
                    "approved": true
                }),
            ),
        )
        .await
        .unwrap();

    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.output, Some(json!({ "approved": true })));
    let signal = completed.signal("approval-delivery-1").unwrap();
    assert_eq!(signal.consumed_by.as_deref(), Some("approval"));
    assert!(signal.payload_as::<Approval>().unwrap().approved);
    assert_eq!(
        completed.signal_waits["approval"].status,
        SignalWaitStatus::Completed
    );
    assert_eq!(
        completed.signal_wait_payload("approval"),
        Some(&json!({ "approved": true }))
    );
    assert!(
        completed
            .signal_wait_payload_as::<Approval>("approval")
            .unwrap()
            .unwrap()
            .approved
    );

    let history = engine.history(&run_id).await.unwrap();
    let event_types = history
        .iter()
        .map(|envelope| envelope.event.event_key())
        .collect::<Vec<_>>();
    assert_eq!(
        event_types,
        vec![
            "flow.run.created",
            "flow.run.started",
            "flow.signal.wait.created",
            "flow.signal.received",
            "flow.signal.wait.completed",
            "flow.run.completed",
        ]
    );
}

struct OrderedSignalRuntime;

#[async_trait]
impl FlowRuntime for OrderedSignalRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.signal_payload("release").is_none() {
            return Ok(context.wait_for_signal("release", RELEASE_SIGNAL));
        }
        if context.signal_payload("approval").is_none() {
            return Ok(context.wait_for_signal("approval", APPROVAL_SIGNAL));
        }
        Ok(context.complete(json!({
            "release": context.signal_payload("release"),
            "approval": context.signal_payload("approval"),
        })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("signal workflow does not execute steps")
    }
}

#[tokio::test]
async fn signals_buffer_until_a_matching_wait_and_preserve_delivery_order() {
    let engine = FlowEngine::in_memory(Arc::new(OrderedSignalRuntime));
    let run_id = engine
        .start_with_id("signal-buffering", spec(), json!({}))
        .await
        .unwrap();

    let still_waiting = engine
        .send_signal(
            &run_id,
            WorkflowSignal::new(
                "approval-delivery-1",
                APPROVAL_SIGNAL,
                json!({ "position": 1 }),
            ),
        )
        .await
        .unwrap();
    assert_eq!(still_waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(
        still_waiting
            .signal("approval-delivery-1")
            .unwrap()
            .consumed_by,
        None
    );

    let completed = engine
        .send_signal(
            &run_id,
            WorkflowSignal::new(
                "release-delivery-1",
                RELEASE_SIGNAL,
                json!({ "position": 2 }),
            ),
        )
        .await
        .unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(
        completed.signal_waits["release"].signal_id.as_deref(),
        Some("release-delivery-1")
    );
    assert_eq!(
        completed.signal_waits["approval"].signal_id.as_deref(),
        Some("approval-delivery-1")
    );
}

#[tokio::test]
async fn signal_redelivery_is_idempotent_and_conflicting_payload_is_rejected() {
    let engine = FlowEngine::in_memory(Arc::new(ApprovalRuntime));
    let run_id = engine
        .start_with_id("signal-idempotency", spec(), json!({}))
        .await
        .unwrap();
    let signal = WorkflowSignal::new(
        "approval-delivery-1",
        APPROVAL_SIGNAL,
        json!({ "approved": true }),
    );

    let first = engine.send_signal(&run_id, signal.clone()).await.unwrap();
    let repeated = engine.send_signal(&run_id, signal).await.unwrap();
    assert_eq!(first.status, WorkflowRunStatus::Completed);
    assert_eq!(repeated.status, WorkflowRunStatus::Completed);
    assert_eq!(
        engine
            .history(&run_id)
            .await
            .unwrap()
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::SignalReceived { .. }))
            .count(),
        1
    );

    let error = engine
        .send_signal(
            &run_id,
            WorkflowSignal::new(
                "approval-delivery-1",
                APPROVAL_SIGNAL,
                json!({ "approved": false }),
            ),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::SignalConflict {
            signal_id,
            ..
        } if signal_id == "approval-delivery-1"
    ));
}

struct ContinueThenSignalRuntime;

#[async_trait]
impl FlowRuntime for ContinueThenSignalRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        let generation = context.input()["generation"].as_u64().unwrap();
        if generation == 0 {
            return Ok(context.continue_as_new(json!({ "generation": 1 })));
        }
        let Some(payload) = context.signal_payload("approval") else {
            return Ok(context.wait_for_signal("approval", APPROVAL_SIGNAL));
        };
        Ok(context.complete(payload.clone()))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("signal workflow does not execute steps")
    }
}

#[tokio::test]
async fn signals_follow_continue_as_new_and_redeliver_against_the_original_run_id() {
    let engine = FlowEngine::in_memory(Arc::new(ContinueThenSignalRuntime));
    let root_run_id = engine
        .start_with_id(
            "signal-continuation-root",
            spec(),
            json!({ "generation": 0 }),
        )
        .await
        .unwrap();
    let chain = engine.continuation_chain(&root_run_id).await.unwrap();
    assert_eq!(chain.len(), 2);
    let leaf_run_id = chain[1].run_id.clone();

    let signal = WorkflowSignal::new(
        "approval-delivery-1",
        APPROVAL_SIGNAL,
        json!({ "approved": true }),
    );
    let completed = engine
        .send_signal(&root_run_id, signal.clone())
        .await
        .unwrap();
    assert_eq!(completed.run_id, leaf_run_id);
    assert_eq!(completed.status, WorkflowRunStatus::Completed);

    let repeated = engine.send_signal(&root_run_id, signal).await.unwrap();
    assert_eq!(repeated.run_id, leaf_run_id);
    assert_eq!(
        engine
            .history(&leaf_run_id)
            .await
            .unwrap()
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::SignalReceived { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn undeclared_and_invalid_signals_are_rejected_without_changing_history() {
    let engine = FlowEngine::in_memory(Arc::new(ApprovalRuntime));
    let run_id = engine
        .start_with_id("signal-validation", spec(), json!({}))
        .await
        .unwrap();
    let original_length = engine.history(&run_id).await.unwrap().len();

    for signal in [
        WorkflowSignal::new("", APPROVAL_SIGNAL, json!({})),
        WorkflowSignal::new("delivery-1", "", json!({})),
        WorkflowSignal::new("delivery-1", "order.unknown", json!({})),
    ] {
        assert!(matches!(
            engine.send_signal(&run_id, signal).await.unwrap_err(),
            FlowError::InvalidTransition(_)
        ));
    }

    assert_eq!(
        engine.history(&run_id).await.unwrap().len(),
        original_length
    );
}

#[tokio::test]
async fn concurrent_redelivery_commits_one_signal_event() {
    let engine = FlowEngine::in_memory(Arc::new(ApprovalRuntime));
    let run_id = engine
        .start_with_id("signal-concurrent-redelivery", spec(), json!({}))
        .await
        .unwrap();
    let signal = WorkflowSignal::new(
        "approval-delivery-1",
        APPROVAL_SIGNAL,
        json!({ "approved": true }),
    );

    let (left, right) = tokio::join!(
        engine.send_signal(&run_id, signal.clone()),
        engine.send_signal(&run_id, signal)
    );
    assert_eq!(left.unwrap().status, WorkflowRunStatus::Completed);
    assert_eq!(right.unwrap().status, WorkflowRunStatus::Completed);
    assert_eq!(
        engine
            .history(&run_id)
            .await
            .unwrap()
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::SignalReceived { .. }))
            .count(),
        1
    );
}

struct FailAfterSignalPairingRuntime {
    fail_once: AtomicBool,
}

#[async_trait]
impl FlowRuntime for FailAfterSignalPairingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        let Some(payload) = context.signal_payload("approval") else {
            return Ok(context.wait_for_signal("approval", APPROVAL_SIGNAL));
        };
        if self.fail_once.swap(false, Ordering::SeqCst) {
            return Err(FlowError::Runtime(
                "simulated process loss after signal pairing".to_string(),
            ));
        }
        Ok(context.complete(payload.clone()))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("signal workflow does not execute steps")
    }
}

#[tokio::test]
async fn redelivery_recovers_after_signal_pairing_commits_before_replay_finishes() {
    let store = Arc::new(a3s_flow::InMemoryEventStore::new());
    let first_engine = FlowEngine::new(
        store.clone(),
        Arc::new(FailAfterSignalPairingRuntime {
            fail_once: AtomicBool::new(true),
        }),
    );
    let run_id = first_engine
        .start_with_id("signal-pairing-recovery", spec(), json!({}))
        .await
        .unwrap();
    let signal = WorkflowSignal::new(
        "approval-delivery-1",
        APPROVAL_SIGNAL,
        json!({ "approved": true }),
    );

    let error = first_engine
        .send_signal(&run_id, signal.clone())
        .await
        .unwrap_err();
    assert!(matches!(error, FlowError::Runtime(message) if message.contains("process loss")));
    let interrupted = first_engine.snapshot(&run_id).await.unwrap();
    assert_eq!(
        interrupted.signal_waits["approval"].status,
        SignalWaitStatus::Completed
    );
    assert_eq!(interrupted.status, WorkflowRunStatus::Running);

    let replacement = FlowEngine::new(
        store,
        Arc::new(FailAfterSignalPairingRuntime {
            fail_once: AtomicBool::new(false),
        }),
    );
    let completed = replacement.send_signal(&run_id, signal).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    let history = replacement.history(&run_id).await.unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::SignalReceived { .. }))
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::SignalWaitCompleted { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn signal_delivery_recovers_a_run_created_without_run_started() {
    let store = Arc::new(a3s_flow::InMemoryEventStore::new());
    store
        .append(
            "signal-pending-start",
            FlowEvent::RunCreated {
                spec: spec(),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    let engine = FlowEngine::new(store, Arc::new(ApprovalRuntime));

    let completed = engine
        .send_signal(
            "signal-pending-start",
            WorkflowSignal::new(
                "approval-delivery-1",
                APPROVAL_SIGNAL,
                json!({ "approved": true }),
            ),
        )
        .await
        .unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(
        engine
            .history("signal-pending-start")
            .await
            .unwrap()
            .iter()
            .map(|envelope| envelope.event.event_key())
            .collect::<Vec<_>>(),
        vec![
            "flow.run.created",
            "flow.run.started",
            "flow.signal.received",
            "flow.signal.wait.created",
            "flow.signal.wait.completed",
            "flow.run.completed",
        ]
    );
}

struct CancellableSignalRuntime;

#[async_trait]
impl FlowRuntime for CancellableSignalRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.cancellation_request().is_some() {
            return Ok(context.cancel());
        }
        Ok(context.wait_for_signal("approval", APPROVAL_SIGNAL))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("signal workflow does not execute steps")
    }
}

#[tokio::test]
async fn cancellation_deactivates_signal_waits_and_rejects_late_delivery() {
    let engine = FlowEngine::in_memory(Arc::new(CancellableSignalRuntime));
    let run_id = engine
        .start_with_id("signal-cancellation", spec(), json!({}))
        .await
        .unwrap();

    let cancelled = engine
        .request_cancellation(
            &run_id,
            a3s_flow::CancellationRequest::new(Some("operator request".to_string())),
        )
        .await
        .unwrap();
    assert_eq!(cancelled.status, WorkflowRunStatus::Cancelled);
    assert_eq!(
        cancelled.signal_waits["approval"].status,
        SignalWaitStatus::Cancelled
    );
    assert_eq!(engine.run_summary().await.unwrap().open_signal_waits, 0);
    assert!(engine
        .list_open_suspensions(chrono::Utc::now())
        .await
        .unwrap()
        .is_empty());

    let error = engine
        .send_signal(
            &run_id,
            WorkflowSignal::new(
                "approval-delivery-late",
                APPROVAL_SIGNAL,
                json!({ "approved": true }),
            ),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, FlowError::RunTerminal(id) if id == run_id));
}

struct TwoApprovalRuntime;

#[async_trait]
impl FlowRuntime for TwoApprovalRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.signal_payload("release").is_none() {
            return Ok(context.wait_for_signal("release", RELEASE_SIGNAL));
        }
        if context.signal_payload("approval-1").is_none() {
            return Ok(context.wait_for_signal("approval-1", APPROVAL_SIGNAL));
        }
        if context.signal_payload("approval-2").is_none() {
            return Ok(context.wait_for_signal("approval-2", APPROVAL_SIGNAL));
        }
        Ok(context.complete(json!({
            "first": context.signal_payload("approval-1"),
            "second": context.signal_payload("approval-2"),
        })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("signal workflow does not execute steps")
    }
}

#[tokio::test]
async fn queued_signals_with_the_same_name_are_consumed_fifo() {
    let engine = FlowEngine::in_memory(Arc::new(TwoApprovalRuntime));
    let run_id = engine
        .start_with_id("signal-fifo", spec(), json!({}))
        .await
        .unwrap();
    for (signal_id, position) in [("approval-delivery-1", 1), ("approval-delivery-2", 2)] {
        engine
            .send_signal(
                &run_id,
                WorkflowSignal::new(signal_id, APPROVAL_SIGNAL, json!({ "position": position })),
            )
            .await
            .unwrap();
    }

    let completed = engine
        .send_signal(
            &run_id,
            WorkflowSignal::new("release-delivery", RELEASE_SIGNAL, json!({})),
        )
        .await
        .unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(
        completed.signal_waits["approval-1"].signal_id.as_deref(),
        Some("approval-delivery-1")
    );
    assert_eq!(
        completed.signal_waits["approval-2"].signal_id.as_deref(),
        Some("approval-delivery-2")
    );
}

struct SignalWaitDriftRuntime {
    drift: AtomicBool,
}

#[async_trait]
impl FlowRuntime for SignalWaitDriftRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let signal_name = if self.drift.load(Ordering::SeqCst) {
            RELEASE_SIGNAL
        } else {
            APPROVAL_SIGNAL
        };
        Ok(invocation
            .context()
            .wait_for_signal("approval", signal_name))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("signal workflow does not execute steps")
    }
}

#[tokio::test]
async fn replay_rejects_signal_wait_name_drift() {
    let runtime = Arc::new(SignalWaitDriftRuntime {
        drift: AtomicBool::new(false),
    });
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine
        .start_with_id("signal-wait-drift", spec(), json!({}))
        .await
        .unwrap();
    runtime.drift.store(true, Ordering::SeqCst);

    let error = engine
        .send_signal(
            &run_id,
            WorkflowSignal::new("approval-delivery", APPROVAL_SIGNAL, json!({})),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::NonDeterministic { reason, .. }
            if reason.contains("signal wait approval name differs")
    ));
}

struct UnsafeContinuationRuntime;

#[async_trait]
impl FlowRuntime for UnsafeContinuationRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.signal_payload("release").is_none() {
            return Ok(context.wait_for_signal("release", RELEASE_SIGNAL));
        }
        Ok(context.continue_as_new(json!({})))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("signal workflow does not execute steps")
    }
}

#[tokio::test]
async fn engine_rejects_continue_as_new_with_an_unconsumed_signal() {
    let engine = FlowEngine::in_memory(Arc::new(UnsafeContinuationRuntime));
    let run_id = engine
        .start_with_id("signal-unsafe-continuation", spec(), json!({}))
        .await
        .unwrap();
    engine
        .send_signal(
            &run_id,
            WorkflowSignal::new("approval-delivery", APPROVAL_SIGNAL, json!({})),
        )
        .await
        .unwrap();

    let error = engine
        .send_signal(
            &run_id,
            WorkflowSignal::new("release-delivery", RELEASE_SIGNAL, json!({})),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::InvalidTransition(message)
            if message.contains("cannot continue as new with unconsumed signal approval-delivery")
    ));
    assert!(!engine
        .history(&run_id)
        .await
        .unwrap()
        .iter()
        .any(|envelope| matches!(envelope.event, FlowEvent::RunContinuedAsNew { .. })));
}

struct SignalBesideOpenWaitRuntime;

#[async_trait]
impl FlowRuntime for SignalBesideOpenWaitRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.signal_payload("approval").is_some() {
            return Ok(context.complete(json!("approved")));
        }
        let armed = invocation
            .history
            .iter()
            .any(|envelope| matches!(envelope.event, FlowEvent::SignalWaitCreated { .. }));
        if context.update("arm").is_some() && !armed {
            return Ok(context.wait_for_signal("approval", APPROVAL_SIGNAL));
        }
        Ok(context.wait_until("pause", "2030-01-01T00:00:00Z".parse().unwrap()))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("signal-beside-wait runtime does not schedule steps")
    }

    async fn run_update(
        &self,
        _invocation: a3s_flow::UpdateInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        Ok(json!({ "armed": true }))
    }
}

fn signal_beside_wait_spec() -> WorkflowSpec {
    spec().with_update("arm")
}

#[tokio::test]
async fn signal_delivery_wakes_workflow_while_timer_wait_is_open() {
    let engine = FlowEngine::in_memory(Arc::new(SignalBesideOpenWaitRuntime));
    engine
        .start_with_id("signal-beside-wait", signal_beside_wait_spec(), json!({}))
        .await
        .unwrap();
    let armed = engine
        .apply_update(
            "signal-beside-wait",
            WorkflowUpdate::new("arm-1", "arm", json!({})),
        )
        .await
        .unwrap();
    assert_eq!(armed.snapshot.status, WorkflowRunStatus::Suspended);
    assert_eq!(
        armed.snapshot.signal_waits["approval"].status,
        SignalWaitStatus::Waiting
    );

    let snapshot = engine
        .send_signal(
            "signal-beside-wait",
            WorkflowSignal::new("delivery-1", APPROVAL_SIGNAL, json!({ "approved": true })),
        )
        .await
        .unwrap();

    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.output, Some(json!("approved")));
    assert_eq!(
        snapshot.signal_waits["approval"].status,
        SignalWaitStatus::Completed
    );
}

struct CrashAfterSignalWaitCompletedStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashAfterSignalWaitCompletedStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashAfterSignalWaitCompletedStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        let envelope = self
            .inner
            .append_if_sequence(run_id, expected_sequence, event.clone())
            .await?;
        if matches!(
            event,
            FlowEvent::SignalWaitCompleted {
                ref wait_id,
                ..
            } if wait_id == "approval"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash after SignalWaitCompleted before observation drive".into(),
            ));
        }
        Ok(envelope)
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

#[tokio::test]
async fn signal_wait_completed_recovery_wakes_via_drive_while_unscoped_timer_is_open() {
    let store = Arc::new(CrashAfterSignalWaitCompletedStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(SignalBesideOpenWaitRuntime));
    engine
        .start_with_id(
            "signal-wait-completed-drive",
            signal_beside_wait_spec(),
            json!({}),
        )
        .await
        .unwrap();
    engine
        .apply_update(
            "signal-wait-completed-drive",
            WorkflowUpdate::new("arm-1", "arm", json!({})),
        )
        .await
        .unwrap();

    store.armed.store(true, Ordering::SeqCst);
    let interrupted = engine
        .send_signal(
            "signal-wait-completed-drive",
            WorkflowSignal::new("delivery-1", APPROVAL_SIGNAL, json!({ "approved": true })),
        )
        .await
        .expect_err("crash after durable SignalWaitCompleted must interrupt");
    assert!(matches!(interrupted, FlowError::Store(_)));

    let mid = engine
        .snapshot("signal-wait-completed-drive")
        .await
        .unwrap();
    assert!(!mid.status.is_terminal());
    assert_eq!(
        mid.signal_waits["approval"].status,
        SignalWaitStatus::Completed
    );
    assert_eq!(mid.waits["pause"].status, a3s_flow::WaitStatus::Waiting);

    let recovered = engine.drive("signal-wait-completed-drive").await.unwrap();
    assert_eq!(
        recovered.output,
        Some(json!("approved")),
        "drive must observe tip SignalWaitCompleted beside an open unscoped timer"
    );
    assert_eq!(recovered.status, WorkflowRunStatus::Completed);
}

struct BufferedSignalBesideOpenWaitRuntime;

#[async_trait]
impl FlowRuntime for BufferedSignalBesideOpenWaitRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.signal_payload("approval").is_some() {
            return Ok(context.complete(json!("approved")));
        }
        let approval_buffered = invocation.history.iter().any(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::SignalReceived { signal }
                    if signal.name == APPROVAL_SIGNAL
            )
        });
        let wait_opened = invocation.history.iter().any(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::SignalWaitCreated { wait_id, .. } if wait_id == "approval"
            )
        });
        if approval_buffered && !wait_opened {
            return Ok(context.wait_for_signal("approval", APPROVAL_SIGNAL));
        }
        Ok(context.wait_until("pause", "2030-01-01T00:00:00Z".parse().unwrap()))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("buffered-signal runtime does not schedule steps")
    }
}

struct CrashAfterSignalReceivedStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashAfterSignalReceivedStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashAfterSignalReceivedStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        let envelope = self
            .inner
            .append_if_sequence(run_id, expected_sequence, event.clone())
            .await?;
        if matches!(event, FlowEvent::SignalReceived { .. })
            && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash after SignalReceived before observation drive".into(),
            ));
        }
        Ok(envelope)
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

#[tokio::test]
async fn signal_received_recovery_wakes_via_drive_while_unscoped_timer_is_open() {
    let store = Arc::new(CrashAfterSignalReceivedStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(BufferedSignalBesideOpenWaitRuntime));
    engine
        .start_with_id("signal-received-drive", spec(), json!({}))
        .await
        .unwrap();
    engine.drive("signal-received-drive").await.unwrap();

    store.armed.store(true, Ordering::SeqCst);
    let interrupted = engine
        .send_signal(
            "signal-received-drive",
            WorkflowSignal::new("delivery-1", APPROVAL_SIGNAL, json!({ "approved": true })),
        )
        .await
        .expect_err("crash after durable SignalReceived must interrupt");
    assert!(
        matches!(interrupted, FlowError::Store(_)),
        "expected store crash, got {interrupted:?}"
    );

    let mid = engine.snapshot("signal-received-drive").await.unwrap();
    assert!(!mid.status.is_terminal());
    assert!(mid
        .signals
        .iter()
        .any(|signal| signal.signal_id == "delivery-1"));
    assert!(mid.signal_waits.is_empty());
    assert_eq!(mid.waits["pause"].status, a3s_flow::WaitStatus::Waiting);

    let recovered = engine.drive("signal-received-drive").await.unwrap();
    assert_eq!(
        recovered.output,
        Some(json!("approved")),
        "drive must observe tip SignalReceived beside an open unscoped timer"
    );
    assert_eq!(recovered.status, WorkflowRunStatus::Completed);
}

#[path = "signals/worker_outcomes.rs"]
mod worker_outcomes;

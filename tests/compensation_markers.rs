use a3s_flow::{
    CompensationMarker, FlowEngine, FlowError, FlowRuntime, JsonValue, RuntimeCommand,
    StepInvocation, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

fn checkout_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "compensation.checkout",
        "1",
        "tests::compensation_markers",
        "main",
    )
}

struct CheckoutRuntime;

#[async_trait]
impl FlowRuntime for CheckoutRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        let reserve = ctx.step_output("reserve-inventory");
        let charge = ctx.step_output("charge-card");
        let release = ctx.step_output("release-inventory");

        if reserve.is_none() {
            return Ok(ctx.schedule_step(
                "reserve-inventory",
                "reserve_inventory",
                json!({
                    "sku": ctx.input()["sku"],
                    "quantity": ctx.input()["quantity"],
                }),
            ));
        }

        if ctx.compensation_marker("reserve").is_none() {
            return Ok(ctx.record_compensation_marker(CompensationMarker::new(
                "reserve",
                "reserve-inventory",
                json!({
                    "reservationId": reserve.unwrap()["reservationId"],
                }),
            )));
        }

        if charge.is_none() {
            return Ok(ctx.schedule_step(
                "charge-card",
                "charge_card",
                json!({
                    "orderId": ctx.input()["orderId"],
                    "reservationId": reserve.unwrap()["reservationId"],
                    "amount": ctx.input()["amount"],
                }),
            ));
        }

        if charge.unwrap()["ok"] == false {
            if release.is_none() {
                let marker = ctx.compensation_marker("reserve").unwrap();
                return Ok(ctx.schedule_step(
                    "release-inventory",
                    "release_inventory",
                    json!({
                        "reservationId": marker.details["reservationId"],
                        "reason": charge.unwrap()["reason"],
                    }),
                ));
            }
            if ctx
                .compensation_marker("reserve")
                .is_some_and(|marker| marker.is_open())
            {
                return Ok(ctx.complete_compensation_marker(
                    "reserve",
                    json!({ "released": true, "step": "release-inventory" }),
                ));
            }
            return Ok(ctx.complete(json!({
                "status": "compensated",
                "openMarkers": ctx.open_compensation_marker_ids(),
            })));
        }

        Ok(ctx.complete(json!({ "status": "completed" })))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
        match invocation.step_name.as_str() {
            "reserve_inventory" => Ok(json!({
                "ok": true,
                "reservationId": "resv-0001",
            })),
            "charge_card" => Ok(json!({
                "ok": false,
                "reason": "card_declined",
            })),
            "release_inventory" => Ok(json!({
                "released": true,
                "reservationId": invocation.input["reservationId"],
            })),
            step => Err(FlowError::Runtime(format!("unknown step: {step}"))),
        }
    }
}

#[tokio::test]
async fn compensation_markers_track_open_obligations_through_cleanup() {
    let engine = FlowEngine::in_memory(Arc::new(CheckoutRuntime));
    let run_id = engine
        .start_with_id(
            "comp-checkout",
            checkout_spec(),
            json!({
                "orderId": "order-1",
                "sku": "shirt",
                "quantity": 1,
                "amount": 4200,
            }),
        )
        .await
        .unwrap();

    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(
        snapshot.output,
        Some(json!({
            "status": "compensated",
            "openMarkers": []
        }))
    );
    let marker = snapshot.compensation_marker("reserve").unwrap();
    assert!(marker.is_completed());
    assert_eq!(marker.compensates, "reserve-inventory");
    assert_eq!(
        marker.outcome,
        Some(json!({ "released": true, "step": "release-inventory" }))
    );
}

#[tokio::test]
async fn compensation_marker_record_is_idempotent_and_rejects_drift() {
    struct DriftRuntime {
        phase: std::sync::atomic::AtomicUsize,
    }
    #[async_trait]
    impl FlowRuntime for DriftRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> a3s_flow::Result<RuntimeCommand> {
            use std::sync::atomic::Ordering;
            let ctx = invocation.context();
            let phase = self.phase.fetch_add(1, Ordering::SeqCst);
            if phase == 0 {
                return Ok(ctx.record_compensation_marker(CompensationMarker::new(
                    "reserve",
                    "reserve-inventory",
                    json!({ "reservationId": "resv-1" }),
                )));
            }
            if phase == 1 {
                return Ok(ctx.record_compensation_marker(CompensationMarker::new(
                    "reserve",
                    "reserve-inventory",
                    json!({ "reservationId": "resv-DRIFT" }),
                )));
            }
            Ok(ctx.complete(json!({})))
        }

        async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
            unreachable!()
        }
    }

    let engine = FlowEngine::in_memory(Arc::new(DriftRuntime {
        phase: std::sync::atomic::AtomicUsize::new(0),
    }));
    let err = engine
        .start_with_id("comp-drift", checkout_spec(), json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, FlowError::NonDeterministic { ref reason, .. } if reason.contains("differs")),
        "{err:?}"
    );
}

#[tokio::test]
async fn compensation_marker_completion_is_idempotent() {
    struct CompleteRuntime {
        completes: std::sync::atomic::AtomicUsize,
    }
    #[async_trait]
    impl FlowRuntime for CompleteRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> a3s_flow::Result<RuntimeCommand> {
            use std::sync::atomic::Ordering;
            let ctx = invocation.context();
            if ctx.compensation_marker("reserve").is_none() {
                return Ok(ctx.record_compensation_marker(CompensationMarker::new(
                    "reserve",
                    "reserve-inventory",
                    json!({}),
                )));
            }
            if ctx
                .compensation_marker("reserve")
                .is_some_and(|marker| marker.is_open())
                || self.completes.fetch_add(1, Ordering::SeqCst) == 0
            {
                return Ok(ctx.complete_compensation_marker("reserve", json!({ "ok": true })));
            }
            Ok(ctx.complete(json!({ "done": true })))
        }

        async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
            unreachable!()
        }
    }

    let engine = FlowEngine::in_memory(Arc::new(CompleteRuntime {
        completes: std::sync::atomic::AtomicUsize::new(0),
    }));
    let run_id = engine
        .start_with_id("comp-complete", checkout_spec(), json!({}))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert!(snapshot
        .compensation_marker("reserve")
        .unwrap()
        .is_completed());
}

/// Workflow-emitted CompensationMarkerRecorded must be observable via ordinary
/// `drive()` recovery when an unscoped timer would otherwise short-circuit.
struct CompensationMarkerBesideOpenWaitRuntime {
    record_phase: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl FlowRuntime for CompensationMarkerBesideOpenWaitRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        use std::sync::atomic::Ordering;
        let ctx = invocation.context();
        if ctx.compensation_marker("reserve").is_some() {
            return Ok(ctx.complete(json!({ "recorded": true })));
        }
        if ctx.wait_status("outer").is_none() {
            return Ok(ctx.wait_until("outer", "2030-01-01T00:00:00Z".parse().unwrap()));
        }
        if self.record_phase.load(Ordering::SeqCst) && ctx.update("arm").is_some() {
            return Ok(ctx.record_compensation_marker(CompensationMarker::new(
                "reserve",
                "reserve-inventory",
                json!({ "reservationId": "resv-probe" }),
            )));
        }
        Ok(ctx.wait_until("outer", "2030-01-01T00:00:00Z".parse().unwrap()))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
        unreachable!("compensation-beside-wait runtime does not schedule steps")
    }

    async fn run_update(
        &self,
        _invocation: a3s_flow::UpdateInvocation,
    ) -> a3s_flow::Result<JsonValue> {
        Ok(json!({ "armed": true }))
    }
}

struct CrashAfterCompensationMarkerRecordedStore {
    inner: a3s_flow::InMemoryEventStore,
    armed: std::sync::atomic::AtomicBool,
}

impl CrashAfterCompensationMarkerRecordedStore {
    fn new() -> Self {
        Self {
            inner: a3s_flow::InMemoryEventStore::new(),
            armed: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl a3s_flow::FlowEventStore for CrashAfterCompensationMarkerRecordedStore {
    async fn append(
        &self,
        run_id: &str,
        event: a3s_flow::FlowEvent,
    ) -> a3s_flow::Result<a3s_flow::FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: a3s_flow::FlowEvent,
    ) -> a3s_flow::Result<a3s_flow::FlowEventEnvelope> {
        use std::sync::atomic::Ordering;
        let envelope = self
            .inner
            .append_if_sequence(run_id, expected_sequence, event.clone())
            .await?;
        if matches!(
            event,
            a3s_flow::FlowEvent::CompensationMarkerRecorded { ref marker, .. }
                if marker.marker_id == "reserve"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash after CompensationMarkerRecorded before observation drive".into(),
            ));
        }
        Ok(envelope)
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<a3s_flow::FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

#[tokio::test]
async fn compensation_marker_recorded_recovery_wakes_via_drive_while_unscoped_timer_is_open() {
    use std::sync::atomic::Ordering;

    let store = Arc::new(CrashAfterCompensationMarkerRecordedStore::new());
    let runtime = Arc::new(CompensationMarkerBesideOpenWaitRuntime {
        record_phase: std::sync::atomic::AtomicBool::new(false),
    });
    let engine = FlowEngine::new(store.clone(), runtime.clone());
    engine
        .start_with_id(
            "comp-marker-drive",
            checkout_spec().with_update("arm"),
            json!({}),
        )
        .await
        .unwrap();
    assert_eq!(
        engine.snapshot("comp-marker-drive").await.unwrap().status,
        WorkflowRunStatus::Suspended
    );

    runtime.record_phase.store(true, Ordering::SeqCst);
    store.armed.store(true, Ordering::SeqCst);
    let interrupted = engine
        .apply_update(
            "comp-marker-drive",
            a3s_flow::WorkflowUpdate::new("arm-1", "arm", json!({})),
        )
        .await
        .expect_err("crash after durable CompensationMarkerRecorded must interrupt");
    assert!(matches!(interrupted, FlowError::Store(_)));

    let mid = engine.snapshot("comp-marker-drive").await.unwrap();
    assert!(!mid.status.is_terminal());
    assert!(mid.compensation_marker("reserve").unwrap().is_open());
    assert_eq!(mid.waits["outer"].status, a3s_flow::WaitStatus::Waiting);

    let recovered = engine.drive("comp-marker-drive").await.unwrap();
    assert_eq!(
        recovered.status,
        WorkflowRunStatus::Completed,
        "drive must observe tip CompensationMarkerRecorded beside an open unscoped timer"
    );
    assert_eq!(recovered.output, Some(json!({ "recorded": true })));
}

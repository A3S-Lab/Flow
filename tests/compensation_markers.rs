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

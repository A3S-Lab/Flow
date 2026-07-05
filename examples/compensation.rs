use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

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

        match (reserve, charge, release) {
            (None, _, _) => Ok(ctx.schedule_step(
                "reserve-inventory",
                "reserve_inventory",
                json!({
                    "sku": ctx.input()["sku"],
                    "quantity": ctx.input()["quantity"],
                }),
            )),
            (Some(reserve), None, _) => Ok(ctx.schedule_step(
                "charge-card",
                "charge_card",
                json!({
                    "orderId": ctx.input()["orderId"],
                    "reservationId": reserve["reservationId"],
                    "amount": ctx.input()["amount"],
                }),
            )),
            (Some(_), Some(charge), None) if charge["ok"] == false => Ok(ctx.schedule_step(
                "release-inventory",
                "release_inventory",
                json!({
                    "reservationId": charge["reservationId"],
                    "reason": charge["reason"],
                }),
            )),
            (Some(reserve), Some(charge), Some(release)) if charge["ok"] == false => Ok(ctx
                .complete(json!({
                    "status": "compensated",
                    "reserve": reserve,
                    "charge": charge,
                    "compensation": release,
                }))),
            (Some(reserve), Some(charge), _) => Ok(ctx.complete(json!({
                "status": "completed",
                "reserve": reserve,
                "charge": charge,
            }))),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "reserve_inventory" => Ok(json!({
                "ok": true,
                "sku": invocation.input["sku"],
                "quantity": invocation.input["quantity"],
                "reservationId": "resv-0001",
            })),
            "charge_card" => Ok(json!({
                "ok": false,
                "orderId": invocation.input["orderId"],
                "reservationId": invocation.input["reservationId"],
                "reason": "card_declined",
            })),
            "release_inventory" => Ok(json!({
                "released": true,
                "reservationId": invocation.input["reservationId"],
                "reason": invocation.input["reason"],
            })),
            step => Err(FlowError::Runtime(format!("unknown step: {step}"))),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(CheckoutRuntime));
    let spec = WorkflowSpec::rust_embedded("examples.checkout", "0.1.0", "examples", "main");

    let run_id = engine
        .start_with_id(
            "checkout-demo-0001",
            spec,
            json!({
                "orderId": "order-0001",
                "sku": "a3s-flow-shirt",
                "quantity": 1,
                "amount": 4200,
            }),
        )
        .await?;
    let snapshot = engine.snapshot(&run_id).await?;

    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.output.as_ref().unwrap()["status"], "compensated");

    println!("run_id={}", snapshot.run_id);
    println!("status={:?}", snapshot.status);
    println!("steps={}", snapshot.steps.len());
    println!(
        "output={}",
        serde_json::to_string_pretty(&snapshot.output).unwrap()
    );
    Ok(())
}

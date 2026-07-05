use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct InvoiceRuntime;

#[async_trait]
impl FlowRuntime for InvoiceRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        let invoice = ctx.step_output("load-invoice");
        let charge = ctx.step_output("charge-card");

        match (invoice, charge) {
            (None, _) => Ok(ctx.schedule_step(
                "load-invoice",
                "load_invoice",
                json!({ "invoiceId": ctx.input()["invoiceId"] }),
            )),
            (Some(invoice), None) => Ok(ctx.schedule_step(
                "charge-card",
                "charge_card",
                json!({
                    "invoiceId": invoice["id"],
                    "amount": invoice["amount"],
                }),
            )),
            (Some(invoice), Some(charge)) => Ok(ctx.complete(json!({
                "invoice": invoice,
                "charge": charge,
            }))),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "load_invoice" => Ok(json!({
                "id": invocation.input["invoiceId"],
                "amount": 4200,
                "currency": "USD",
            })),
            "charge_card" => Ok(json!({
                "status": "authorized",
                "amount": invocation.input["amount"],
            })),
            step => Err(FlowError::Runtime(format!("unknown step: {step}"))),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(InvoiceRuntime));
    let spec = WorkflowSpec::rust_embedded("examples.invoice", "0.1.0", "examples", "main");

    let run_id = engine
        .start_with_id(
            "invoice-demo-0001",
            spec,
            json!({ "invoiceId": "inv-0001" }),
        )
        .await?;
    let snapshot = engine.snapshot(&run_id).await?;

    println!("run_id={}", snapshot.run_id);
    println!("status={:?}", snapshot.status);
    println!(
        "output={}",
        serde_json::to_string_pretty(&snapshot.output).unwrap()
    );
    Ok(())
}

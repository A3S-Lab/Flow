use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, FlowScheduler, FlowWorker, InMemoryFlowTaskQueue,
    RetryPolicy, RuntimeCommand, StepInvocation, StepStatus, WorkflowInvocation, WorkflowRunStatus,
    WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
struct PaymentRuntime {
    attempts: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for PaymentRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if let Some(charge) = ctx.step_output("charge-card") {
            return Ok(ctx.complete(charge.clone()));
        }

        Ok(ctx.schedule_step_with_retry(
            "charge-card",
            "charge_card",
            json!({
                "invoiceId": ctx.input()["invoiceId"],
                "amount": ctx.input()["amount"],
            }),
            RetryPolicy::fixed(2, Duration::from_secs(60)),
        ))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "charge_card" => {
                let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
                if attempt == 1 {
                    return Err(FlowError::Runtime(
                        "processor temporarily unavailable".to_string(),
                    ));
                }

                Ok(json!({
                    "invoiceId": invocation.input["invoiceId"],
                    "status": "authorized",
                    "attempt": attempt,
                }))
            }
            step => Err(FlowError::Runtime(format!("unknown step: {step}"))),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let runtime = Arc::new(PaymentRuntime::default());
    let engine = FlowEngine::in_memory(runtime.clone());
    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine.clone(), queue.clone());
    let worker = FlowWorker::new(engine.clone(), queue);
    let spec = WorkflowSpec::rust_embedded("examples.retry", "0.1.0", "examples", "main");

    let run_id = engine
        .start(
            spec,
            json!({
                "invoiceId": "inv-0002",
                "amount": 1200,
            }),
        )
        .await?;
    let waiting = engine.snapshot(&run_id).await?;
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(waiting.steps["charge-card"].status, StepStatus::Pending);
    assert!(waiting.steps["charge-card"].retry_after.is_some());

    let not_due = scheduler.enqueue_due_work(Utc::now()).await?;
    assert!(!not_due.has_due_work());

    let due_at = Utc::now() + ChronoDuration::seconds(120);
    let tick = scheduler.enqueue_due_work(due_at).await?;
    let outcomes = worker.run_until_idle().await?;
    let completed = engine.snapshot(&run_id).await?;

    println!("initial_status={:?}", waiting.status);
    println!("due_retries={:?}", tick.due_retries);
    println!("worker_outcomes={}", outcomes.len());
    println!("attempts={}", runtime.attempts.load(Ordering::SeqCst));
    println!("final_status={:?}", completed.status);
    println!(
        "output={}",
        serde_json::to_string_pretty(&completed.output).unwrap()
    );

    Ok(())
}

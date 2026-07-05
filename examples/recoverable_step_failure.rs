use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RetryPolicy, RuntimeCommand, StepFailureAction,
    StepInvocation, StepStatus, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct ReportRuntime;

#[async_trait]
impl FlowRuntime for ReportRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();

        if let Some(fallback) = ctx.step_output("cached-report") {
            return Ok(ctx.complete(json!({
                "status": "degraded",
                "report": fallback,
            })));
        }

        if let Some(primary_error) = ctx.step_failed("fresh-report") {
            return Ok(ctx.schedule_step(
                "cached-report",
                "load_cached_report",
                json!({ "freshReportError": primary_error }),
            ));
        }

        if let Some(report) = ctx.step_output("fresh-report") {
            return Ok(ctx.complete(json!({
                "status": "fresh",
                "report": report,
            })));
        }

        Ok(ctx.schedule_step_with_retry(
            "fresh-report",
            "load_fresh_report",
            json!({ "reportId": ctx.input()["reportId"] }),
            RetryPolicy::fixed(2, std::time::Duration::from_millis(0))
                .continue_workflow_on_failure(),
        ))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "load_fresh_report" => Err(FlowError::Runtime(
                "fresh report service unavailable".to_string(),
            )),
            "load_cached_report" => Ok(json!({
                "source": "cache",
                "reportId": "report-0001",
                "reason": invocation.input["freshReportError"],
            })),
            step => Err(FlowError::Runtime(format!("unknown step: {step}"))),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(ReportRuntime));
    let spec = WorkflowSpec::rust_embedded(
        "examples.recoverable-step-failure",
        "0.1.0",
        "examples",
        "main",
    );

    let run_id = engine
        .start_with_id(
            "recoverable-step-failure-demo-0001",
            spec,
            json!({ "reportId": "report-0001" }),
        )
        .await?;
    let snapshot = engine.snapshot(&run_id).await?;

    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.steps["fresh-report"].status, StepStatus::Failed);
    assert_eq!(
        snapshot.steps["fresh-report"].retry.on_exhausted,
        StepFailureAction::ContinueWorkflow
    );
    assert_eq!(
        snapshot.steps["cached-report"].status,
        StepStatus::Completed
    );

    println!("run_id={}", snapshot.run_id);
    println!("status={:?}", snapshot.status);
    println!(
        "fresh_report_error={}",
        snapshot.steps["fresh-report"].error.as_deref().unwrap()
    );
    println!(
        "output={}",
        serde_json::to_string_pretty(&snapshot.output).unwrap()
    );

    Ok(())
}

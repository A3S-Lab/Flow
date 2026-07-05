use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::Arc;

struct InspectionRuntime;

#[async_trait]
impl FlowRuntime for InspectionRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        match ctx.input()["mode"].as_str() {
            Some("complete") => {
                if let Some(output) = ctx.step_output("finish") {
                    return Ok(ctx.complete(output.clone()));
                }
                Ok(ctx.schedule_step(
                    "finish",
                    "finish_report",
                    json!({ "label": ctx.input()["label"] }),
                ))
            }
            Some("wait") => {
                let resume_at = ctx.input()["resumeAt"]
                    .as_str()
                    .ok_or_else(|| FlowError::Runtime("missing resumeAt".to_string()))?
                    .parse::<DateTime<Utc>>()
                    .map_err(|err| FlowError::Runtime(format!("invalid resumeAt: {err}")))?;
                Ok(ctx.wait_until("inspection-wait", resume_at))
            }
            Some("fail") => Ok(ctx.fail("inspection example failure")),
            other => Err(FlowError::Runtime(format!("unknown mode: {other:?}"))),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "finish_report" => Ok(json!({
                "label": invocation.input["label"],
                "finished": true,
            })),
            step => Err(FlowError::Runtime(format!("unknown step: {step}"))),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(InspectionRuntime));
    let spec = WorkflowSpec::rust_embedded("examples.run-inspection", "0.1.0", "examples", "main");

    engine
        .start_with_id(
            "inspect-completed",
            spec.clone(),
            json!({ "mode": "complete", "label": "inventory" }),
        )
        .await?;
    engine
        .start_with_id(
            "inspect-suspended",
            spec.clone(),
            json!({
                "mode": "wait",
                "resumeAt": (now + ChronoDuration::hours(1)).to_rfc3339(),
            }),
        )
        .await?;
    engine
        .start_with_id(
            "inspect-cancelled",
            spec.clone(),
            json!({
                "mode": "wait",
                "resumeAt": (now + ChronoDuration::hours(2)).to_rfc3339(),
            }),
        )
        .await?;
    engine
        .cancel("inspect-cancelled", Some("not needed".to_string()))
        .await?;
    engine
        .start_with_id("inspect-failed", spec, json!({ "mode": "fail" }))
        .await?;

    let run_ids = engine.list_run_ids().await?;
    let snapshots = engine.list_snapshots().await?;
    let failed_history = engine.history("inspect-failed").await?;

    println!("run_ids={run_ids:?}");
    println!("snapshots:");
    for snapshot in &snapshots {
        println!(
            "  {} status={:?} steps={} waits={} error={:?}",
            snapshot.run_id,
            snapshot.status,
            snapshot.steps.len(),
            snapshot.waits.len(),
            snapshot.error
        );
    }
    println!(
        "failed_history_keys={:?}",
        failed_history
            .iter()
            .map(|event| event.event.event_key())
            .collect::<Vec<_>>()
    );

    assert_eq!(
        run_ids,
        vec![
            "inspect-cancelled".to_string(),
            "inspect-completed".to_string(),
            "inspect-failed".to_string(),
            "inspect-suspended".to_string(),
        ]
    );
    assert!(snapshots
        .iter()
        .any(|snapshot| snapshot.status == WorkflowRunStatus::Completed));
    assert!(snapshots
        .iter()
        .any(|snapshot| snapshot.status == WorkflowRunStatus::Suspended));
    assert!(snapshots
        .iter()
        .any(|snapshot| snapshot.status == WorkflowRunStatus::Cancelled));
    assert!(snapshots
        .iter()
        .any(|snapshot| snapshot.status == WorkflowRunStatus::Failed));
    Ok(())
}

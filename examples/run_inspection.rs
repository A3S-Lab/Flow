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
            Some("hook") => {
                if let Some(payload) = ctx.hook_payload("approval") {
                    return Ok(ctx.complete(json!({ "approval": payload })));
                }
                Ok(ctx.create_hook(
                    "approval",
                    ctx.input()["token"].as_str().unwrap_or("inspection-token"),
                    json!({ "kind": "approval" }),
                ))
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
            "inspect-hook",
            spec.clone(),
            json!({ "mode": "hook", "token": "inspection-token" }),
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
    let summary = engine.run_summary().await?;
    let suspensions = engine.list_open_suspensions(now).await?;
    let active_hooks = engine.list_active_hooks().await?;
    let failed_history = engine.history("inspect-failed").await?;

    println!("run_ids={run_ids:?}");
    println!("snapshots:");
    for snapshot in &snapshots {
        println!(
            "  {} status={:?} steps={} waits={} hooks={} error={:?}",
            snapshot.run_id,
            snapshot.status,
            snapshot.steps.len(),
            snapshot.waits.len(),
            snapshot.hooks.len(),
            snapshot.error
        );
    }
    println!("active_hooks:");
    for active in &active_hooks {
        println!(
            "  {} {} token={}",
            active.run_id, active.hook.hook_id, active.hook.token
        );
    }
    println!("open_suspensions:");
    for suspension in &suspensions {
        println!(
            "  {} {} due={}",
            suspension.run_id(),
            suspension.subject_id(),
            suspension.is_due()
        );
    }
    println!(
        "summary total={} suspended={} completed={} failed={} cancelled={} open_waits={} active_hooks={}",
        summary.total_runs,
        summary.suspended_runs,
        summary.completed_runs,
        summary.failed_runs,
        summary.cancelled_runs,
        summary.open_waits,
        summary.active_hooks
    );
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
            "inspect-hook".to_string(),
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
    assert_eq!(active_hooks.len(), 1);
    assert_eq!(active_hooks[0].run_id, "inspect-hook");
    assert_eq!(active_hooks[0].hook.hook_id, "approval");
    assert_eq!(active_hooks[0].hook.token, "inspection-token");
    assert_eq!(
        suspensions
            .iter()
            .map(|suspension| (
                suspension.run_id(),
                suspension.subject_id(),
                suspension.is_due()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("inspect-hook", "approval", false),
            ("inspect-suspended", "inspection-wait", false),
        ]
    );
    assert_eq!(summary.total_runs, 5);
    assert_eq!(summary.suspended_runs, 2);
    assert_eq!(summary.completed_runs, 1);
    assert_eq!(summary.failed_runs, 1);
    assert_eq!(summary.cancelled_runs, 1);
    assert_eq!(summary.open_waits, 1);
    assert_eq!(summary.active_hooks, 1);
    Ok(())
}

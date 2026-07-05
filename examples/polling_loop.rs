use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowRuntime, FlowScheduler, FlowWorker,
    InMemoryFlowTaskQueue, RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowRunStatus,
    WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Default)]
struct ImportRuntime {
    polls: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for ImportRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        let Some(job) = ctx.step_output("start-import") else {
            return Ok(ctx.schedule_step(
                "start-import",
                "start_import",
                json!({ "source": ctx.input()["source"] }),
            ));
        };

        for attempt in 1..=3 {
            let step_id = format!("poll-{attempt}");
            if let Some(status) = ctx.step_output(&step_id) {
                if status["state"] == "ready" {
                    return Ok(ctx.complete(json!({
                        "jobId": job["jobId"],
                        "polls": attempt,
                        "rows": status["rows"],
                    })));
                }
                continue;
            }

            if attempt == 1 {
                return Ok(ctx.schedule_step(
                    step_id,
                    "poll_import",
                    json!({ "jobId": job["jobId"], "attempt": attempt }),
                ));
            }

            let wait_id = format!("wait-poll-{attempt}");
            if !ctx.wait_completed(&wait_id) {
                let resume_at = next_poll_at(&invocation, attempt - 1)?;
                return Ok(ctx.wait_until(wait_id, resume_at));
            }

            return Ok(ctx.schedule_step(
                step_id,
                "poll_import",
                json!({ "jobId": job["jobId"], "attempt": attempt }),
            ));
        }

        Ok(ctx.fail("import did not become ready after 3 polls"))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "start_import" => Ok(json!({
                "jobId": format!("job-{}", invocation.input["source"].as_str().unwrap_or("unknown")),
            })),
            "poll_import" => {
                let attempt = self.polls.fetch_add(1, Ordering::SeqCst) + 1;
                if attempt < 3 {
                    return Ok(json!({
                        "state": "pending",
                        "nextPollAt": (Utc::now() - ChronoDuration::seconds(1)).to_rfc3339(),
                    }));
                }

                Ok(json!({
                    "state": "ready",
                    "rows": 42,
                }))
            }
            step => Err(FlowError::Runtime(format!("unknown step: {step}"))),
        }
    }
}

fn next_poll_at(
    invocation: &WorkflowInvocation,
    attempt: usize,
) -> a3s_flow::Result<DateTime<Utc>> {
    let step_id = format!("poll-{attempt}");
    let output = invocation
        .history
        .iter()
        .find_map(|envelope| match &envelope.event {
            FlowEvent::StepCompleted {
                step_id: id,
                output,
            } if id == &step_id => Some(output),
            _ => None,
        });
    let Some(output) = output else {
        return Err(FlowError::Runtime(format!("missing output for {step_id}")));
    };
    let Some(next_poll_at) = output["nextPollAt"].as_str() else {
        return Err(FlowError::Runtime(format!(
            "missing nextPollAt for {step_id}"
        )));
    };
    next_poll_at
        .parse::<DateTime<Utc>>()
        .map_err(|err| FlowError::Runtime(format!("invalid nextPollAt for {step_id}: {err}")))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let runtime = Arc::new(ImportRuntime::default());
    let engine = FlowEngine::in_memory(runtime.clone());
    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine.clone(), queue.clone());
    let worker = FlowWorker::new(engine.clone(), queue);
    let spec = WorkflowSpec::rust_embedded("examples.polling", "0.1.0", "examples", "main");

    let run_id = engine
        .start(spec, json!({ "source": "customers.csv" }))
        .await?;
    let mut snapshot = engine.snapshot(&run_id).await?;
    let mut scheduler_ticks = 0usize;
    let mut worker_outcomes = 0usize;

    while snapshot.status != WorkflowRunStatus::Completed {
        let tick = scheduler.enqueue_due_work(Utc::now()).await?;
        scheduler_ticks += 1;
        if tick.has_due_work() {
            worker_outcomes += worker.run_until_idle().await?.len();
        }
        snapshot = engine.snapshot(&run_id).await?;
    }

    println!("scheduler_ticks={scheduler_ticks}");
    println!("worker_outcomes={worker_outcomes}");
    println!("poll_steps={}", runtime.polls.load(Ordering::SeqCst));
    println!("status={:?}", snapshot.status);
    println!(
        "output={}",
        serde_json::to_string_pretty(&snapshot.output).unwrap()
    );

    Ok(())
}

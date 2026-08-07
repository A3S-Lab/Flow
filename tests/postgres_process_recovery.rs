#![cfg(feature = "postgres")]

use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, FlowRuntime, FlowTask,
    FlowTaskQueue, FlowWorker, PostgresEventStore, PostgresFlowTaskQueue, RetryPolicy,
    RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use std::future::pending;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use uuid::Uuid;

const PROBE_TEST: &str = "postgres_worker_death_probe";
const PROBE_PARENT_ENV: &str = "A3S_FLOW_PROCESS_PROBE_PARENT";
const PROBE_POSTGRES_ENV: &str = "A3S_FLOW_PROCESS_PROBE_POSTGRES";
const PROBE_QUEUE_ENV: &str = "A3S_FLOW_PROCESS_PROBE_QUEUE";
const PROBE_RUN_ENV: &str = "A3S_FLOW_PROCESS_PROBE_RUN";
const PROBE_STATE_ENV: &str = "A3S_FLOW_PROCESS_PROBE_STATE";
const PROBE_MARKER_ENV: &str = "A3S_FLOW_PROCESS_PROBE_MARKER";

fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.postgres-process-recovery",
        "1",
        "tests::postgres_process_recovery",
        "main",
    )
}

#[derive(Clone)]
struct ProcessRecoveryRuntime {
    state_dir: PathBuf,
}

impl ProcessRecoveryRuntime {
    fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
        }
    }

    fn effect_path(&self) -> PathBuf {
        self.state_dir.join("logical-effect.txt")
    }

    fn attempts_path(&self) -> PathBuf {
        self.state_dir.join("physical-attempts.txt")
    }
}

#[async_trait]
impl FlowRuntime for ProcessRecoveryRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match context.step_output("durable-effect") {
            Some(output) => Ok(context.complete(output.clone())),
            None => Ok(context.schedule_step_with_retry(
                "durable-effect",
                "commitDurableEffect",
                json!({
                    "idempotencyKey": format!("{}:durable-effect", context.run_id()),
                }),
                RetryPolicy::none(),
            )),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        tokio::fs::create_dir_all(&self.state_dir).await?;
        let mut attempts = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.attempts_path())
            .await?;
        attempts.write_all(b"attempt\n").await?;
        attempts.flush().await?;
        attempts.sync_data().await?;

        match tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(self.effect_path())
            .await
        {
            Ok(mut effect) => {
                effect
                    .write_all(
                        invocation.input["idempotencyKey"]
                            .as_str()
                            .unwrap()
                            .as_bytes(),
                    )
                    .await?;
                effect.flush().await?;
                effect.sync_data().await?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(FlowError::Io(error)),
        }
        Ok(json!({ "committed": true }))
    }
}

struct PauseBeforeCompletionStore {
    inner: PostgresEventStore,
    marker: PathBuf,
    lease_id: String,
}

#[async_trait]
impl FlowEventStore for PauseBeforeCompletionStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if matches!(
            &event,
            FlowEvent::StepCompleted { step_id, .. } if step_id == "durable-effect"
        ) {
            publish_marker(&self.marker, run_id, expected_sequence, &self.lease_id).await?;
            pending::<()>().await;
            unreachable!("process-death probe resumed after its crash boundary")
        }
        self.inner
            .append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

async fn publish_marker(
    marker: &Path,
    run_id: &str,
    expected_sequence: u64,
    lease_id: &str,
) -> a3s_flow::Result<()> {
    let temporary = marker.with_extension(format!("{}.tmp", std::process::id()));
    tokio::fs::write(
        &temporary,
        serde_json::to_vec(&json!({
            "runId": run_id,
            "expectedSequence": expected_sequence,
            "leaseId": lease_id,
        }))?,
    )
    .await?;
    tokio::fs::rename(temporary, marker).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "private subprocess used only by the PostgreSQL process-death gate"]
async fn postgres_worker_death_probe() {
    assert_eq!(std::env::var(PROBE_PARENT_ENV).as_deref(), Ok("1"));
    let postgres_url = std::env::var(PROBE_POSTGRES_ENV).unwrap();
    let queue_name = std::env::var(PROBE_QUEUE_ENV).unwrap();
    let run_id = std::env::var(PROBE_RUN_ENV).unwrap();
    let state_dir = PathBuf::from(std::env::var(PROBE_STATE_ENV).unwrap());
    let marker = PathBuf::from(std::env::var(PROBE_MARKER_ENV).unwrap());
    let queue = PostgresFlowTaskQueue::connect_with_queue(&postgres_url, queue_name)
        .await
        .unwrap();
    let lease = queue.lease().await.unwrap().expect("probe task lease");
    let store = PostgresEventStore::connect(&postgres_url).await.unwrap();
    let paused = Arc::new(PauseBeforeCompletionStore {
        inner: store,
        marker,
        lease_id: lease.lease_id,
    });
    let engine = FlowEngine::new(paused, Arc::new(ProcessRecoveryRuntime::new(state_dir)));
    let worker = FlowWorker::new(engine, Arc::new(queue));
    worker.handle(lease.task).await.unwrap();
    panic!("process-death probe returned before being killed for run {run_id}");
}

#[tokio::test]
async fn postgres_worker_recovers_after_process_death_reconnect_and_lease_expiry() {
    let Some(postgres_url) = postgres_url_from_env() else {
        eprintln!("skipping PostgreSQL process-death test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let scope = Uuid::new_v4();
    let queue_name = format!("process-death-{scope}");
    let run_id = format!("process-death-{scope}");
    let state = tempfile::tempdir().unwrap();
    let marker = state.path().join("completion-boundary.json");
    let store = PostgresEventStore::connect(&postgres_url).await.unwrap();
    let created = store
        .append_if_sequence(
            &run_id,
            0,
            FlowEvent::RunCreated {
                spec: workflow_spec(),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store
        .append_if_sequence(&run_id, created.sequence, FlowEvent::RunStarted)
        .await
        .unwrap();
    let queue = PostgresFlowTaskQueue::connect_with_queue(&postgres_url, &queue_name)
        .await
        .unwrap();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: run_id.clone(),
        })
        .await
        .unwrap();

    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .arg(PROBE_TEST)
        .arg("--exact")
        .arg("--ignored")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(PROBE_PARENT_ENV, "1")
        .env(PROBE_POSTGRES_ENV, &postgres_url)
        .env(PROBE_QUEUE_ENV, &queue_name)
        .env(PROBE_RUN_ENV, &run_id)
        .env(PROBE_STATE_ENV, state.path())
        .env(PROBE_MARKER_ENV, &marker)
        .kill_on_drop(true);
    let mut probe = command.spawn().unwrap();
    wait_for_marker(&mut probe, &marker).await;
    let document: serde_json::Value =
        serde_json::from_slice(&tokio::fs::read(&marker).await.unwrap()).unwrap();
    assert_eq!(document["runId"], run_id);
    assert!(document["expectedSequence"].as_u64().is_some());
    let stale_lease_id = document["leaseId"].as_str().unwrap().to_string();

    probe.kill().await.unwrap();
    let status = probe.wait().await.unwrap();
    assert!(!status.success());
    let interrupted_history = store.list(&run_id).await.unwrap();
    assert!(interrupted_history
        .iter()
        .any(|event| matches!(event.event, FlowEvent::StepStarted { .. })));
    assert!(!interrupted_history
        .iter()
        .any(|event| matches!(event.event, FlowEvent::StepCompleted { .. })));

    let reconnected_queue = Arc::new(
        PostgresFlowTaskQueue::connect_with_queue(&postgres_url, &queue_name)
            .await
            .unwrap(),
    );
    assert_eq!(
        reconnected_queue
            .requeue_inflight_older_than(Utc::now() + ChronoDuration::seconds(1))
            .await
            .unwrap(),
        1
    );
    let stale_ack = reconnected_queue.ack(&stale_lease_id).await.unwrap_err();
    assert!(matches!(
        stale_ack,
        FlowError::LeaseLost(lease_id) if lease_id == stale_lease_id
    ));

    let reconnected_store = Arc::new(PostgresEventStore::connect(&postgres_url).await.unwrap());
    let replacement_engine = FlowEngine::new(
        reconnected_store.clone(),
        Arc::new(ProcessRecoveryRuntime::new(state.path())),
    );
    let replacement = FlowWorker::new(replacement_engine.clone(), reconnected_queue.clone());
    replacement
        .run_once()
        .await
        .unwrap()
        .expect("replayed task");

    let snapshot = replacement_engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(reconnected_queue.len().await.unwrap(), 0);
    assert_eq!(reconnected_queue.inflight_len().await.unwrap(), 0);
    let recovered_history = reconnected_store.list(&run_id).await.unwrap();
    assert_eq!(
        recovered_history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::StepStarted { .. }))
            .count(),
        1,
        "replacement must redeliver the interrupted attempt instead of consuming retry budget"
    );
    assert_eq!(
        recovered_history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::StepCompleted { .. }))
            .count(),
        1
    );
    assert_eq!(
        tokio::fs::read_to_string(state.path().join("physical-attempts.txt"))
            .await
            .unwrap()
            .lines()
            .count(),
        2
    );
    assert_eq!(
        tokio::fs::read_to_string(state.path().join("logical-effect.txt"))
            .await
            .unwrap(),
        format!("{run_id}:durable-effect")
    );
}

async fn wait_for_marker(probe: &mut tokio::process::Child, marker: &Path) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if marker.is_file() {
            return;
        }
        if let Some(status) = probe.try_wait().unwrap() {
            panic!("process-death probe exited with {status} before publishing its marker");
        }
        assert!(
            Instant::now() < deadline,
            "process-death probe did not publish its marker within 60 seconds"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

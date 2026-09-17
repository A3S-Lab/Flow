//! FLOW-R2 real PostgreSQL process-death gates for Activity create→start,
//! heartbeat, and unknown-outcome append boundaries.
//!
//! Completion-boundary coverage remains in `postgres_process_recovery.rs`.
//! These probes kill the worker process after publishing a marker and before
//! the target event becomes durable, then prove lease expiry + replacement
//! recovery without inventing the interrupted event.

#![cfg(feature = "postgres")]

use a3s_flow::{
    ActivityInvocation, ActivityStatus, FlowEngine, FlowError, FlowEvent, FlowEventEnvelope,
    FlowEventStore, FlowRuntime, FlowTask, FlowTaskQueue, FlowWorker, PostgresEventStore,
    PostgresFlowTaskQueue, RetryPolicy, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowRunStatus, WorkflowSpec,
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

const START_PROBE_TEST: &str = "postgres_activity_start_death_probe";
const HEARTBEAT_PROBE_TEST: &str = "postgres_activity_heartbeat_death_probe";
const UNKNOWN_PROBE_TEST: &str = "postgres_activity_unknown_death_probe";
const HEARTBEAT_THEN_COMPLETION_PROBE_TEST: &str =
    "postgres_activity_heartbeat_then_completion_death_probe";
const PROBE_PARENT_ENV: &str = "A3S_FLOW_ACTIVITY_BOUNDARY_PROBE_PARENT";
const PROBE_POSTGRES_ENV: &str = "A3S_FLOW_ACTIVITY_BOUNDARY_PROBE_POSTGRES";
const PROBE_QUEUE_ENV: &str = "A3S_FLOW_ACTIVITY_BOUNDARY_PROBE_QUEUE";
const PROBE_RUN_ENV: &str = "A3S_FLOW_ACTIVITY_BOUNDARY_PROBE_RUN";
const PROBE_STATE_ENV: &str = "A3S_FLOW_ACTIVITY_BOUNDARY_PROBE_STATE";
const PROBE_MARKER_ENV: &str = "A3S_FLOW_ACTIVITY_BOUNDARY_PROBE_MARKER";
const PROBE_BOUNDARY_ENV: &str = "A3S_FLOW_ACTIVITY_BOUNDARY_PROBE_BOUNDARY";

fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.postgres-activity-boundary-process-recovery",
        "1",
        "tests::postgres_activity_boundary_process_recovery",
        "main",
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CrashBoundary {
    Started,
    Heartbeat,
    Unknown,
    /// Durable heartbeat first, then crash before ActivityCompleted.
    CompletionAfterHeartbeat,
}

impl CrashBoundary {
    fn from_env() -> Self {
        match std::env::var(PROBE_BOUNDARY_ENV).as_deref() {
            Ok("heartbeat") => Self::Heartbeat,
            Ok("unknown") => Self::Unknown,
            Ok("completion-after-heartbeat") => Self::CompletionAfterHeartbeat,
            _ => Self::Started,
        }
    }

    fn as_env(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Heartbeat => "heartbeat",
            Self::Unknown => "unknown",
            Self::CompletionAfterHeartbeat => "completion-after-heartbeat",
        }
    }

    fn probe_test(self) -> &'static str {
        match self {
            Self::Started => START_PROBE_TEST,
            Self::Heartbeat => HEARTBEAT_PROBE_TEST,
            Self::Unknown => UNKNOWN_PROBE_TEST,
            Self::CompletionAfterHeartbeat => HEARTBEAT_THEN_COMPLETION_PROBE_TEST,
        }
    }

    fn matches_event(self, event: &FlowEvent) -> bool {
        match self {
            Self::Started => {
                matches!(event, FlowEvent::ActivityStarted { activity_id, .. } if activity_id == "durable-effect")
            }
            Self::Heartbeat => {
                matches!(event, FlowEvent::ActivityHeartbeat { activity_id, .. } if activity_id == "durable-effect")
            }
            Self::Unknown => {
                matches!(event, FlowEvent::ActivityUnknown { activity_id, .. } if activity_id == "durable-effect")
            }
            Self::CompletionAfterHeartbeat => {
                matches!(event, FlowEvent::ActivityCompleted { activity_id, .. } if activity_id == "durable-effect")
            }
        }
    }
}

struct PauseBeforeBoundaryStore {
    inner: PostgresEventStore,
    boundary: CrashBoundary,
    marker: PathBuf,
    lease_id: String,
}

#[async_trait]
impl FlowEventStore for PauseBeforeBoundaryStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if self.boundary.matches_event(&event) {
            publish_marker(&self.marker, run_id, expected_sequence, &self.lease_id).await?;
            pending::<()>().await;
            unreachable!("activity-boundary process-death probe resumed after its crash boundary")
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

#[derive(Clone)]
struct BoundaryRecoveryRuntime {
    state_dir: PathBuf,
    boundary: CrashBoundary,
}

impl BoundaryRecoveryRuntime {
    fn new(state_dir: impl Into<PathBuf>, boundary: CrashBoundary) -> Self {
        Self {
            state_dir: state_dir.into(),
            boundary,
        }
    }

    fn effect_path(&self) -> PathBuf {
        self.state_dir.join("logical-effect.txt")
    }

    fn attempts_path(&self) -> PathBuf {
        self.state_dir.join("physical-attempts.txt")
    }

    fn release_path(&self) -> PathBuf {
        self.state_dir.join("release-after-heartbeat")
    }

    async fn attempt_count(&self) -> usize {
        tokio::fs::read_to_string(self.attempts_path())
            .await
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.is_empty())
            .count()
    }

    async fn record_attempt(&self) -> a3s_flow::Result<()> {
        tokio::fs::create_dir_all(&self.state_dir).await?;
        let mut attempts = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.attempts_path())
            .await?;
        attempts.write_all(b"attempt\n").await?;
        attempts.flush().await?;
        attempts.sync_data().await?;
        Ok(())
    }

    async fn write_durable_effect(&self, input: &serde_json::Value) -> a3s_flow::Result<()> {
        match tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(self.effect_path())
            .await
        {
            Ok(mut effect) => {
                effect
                    .write_all(input["idempotencyKey"].as_str().unwrap().as_bytes())
                    .await?;
                effect.flush().await?;
                effect.sync_data().await?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(FlowError::Io(error)),
        }
        Ok(())
    }

    async fn commit_durable_effect(
        &self,
        input: &serde_json::Value,
    ) -> a3s_flow::Result<serde_json::Value> {
        self.record_attempt().await?;
        self.write_durable_effect(input).await?;
        Ok(json!({ "committed": true }))
    }
}

#[async_trait]
impl FlowRuntime for BoundaryRecoveryRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match context.activity_output("durable-effect") {
            Some(output) => Ok(context.complete(output.clone())),
            None => Ok(context.schedule_activity_with_retry(
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
        self.commit_durable_effect(&invocation.input).await
    }

    async fn run_activity(
        &self,
        invocation: ActivityInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        match self.boundary {
            CrashBoundary::Started => self.commit_durable_effect(&invocation.input).await,
            CrashBoundary::Heartbeat => {
                self.record_attempt().await?;
                if self.attempt_count().await == 1 {
                    // Stay running so the probe can attempt a durable heartbeat.
                    pending::<()>().await;
                    unreachable!("heartbeat wait resumed without process death")
                }
                // Redelivery after process death: finish the durable effect.
                self.write_durable_effect(&invocation.input).await?;
                Ok(json!({ "committed": true }))
            }
            CrashBoundary::Unknown => {
                self.record_attempt().await?;
                if self.attempt_count().await == 1 {
                    return Err(FlowError::UnknownOutcome(
                        "provider connection lost after request".to_string(),
                    ));
                }
                self.write_durable_effect(&invocation.input).await?;
                Ok(json!({ "committed": true }))
            }
            CrashBoundary::CompletionAfterHeartbeat => {
                self.record_attempt().await?;
                let attempts = self.attempt_count().await;
                let wait_marker = if attempts == 1 {
                    Some(self.release_path())
                } else if attempts == 2 {
                    Some(self.state_dir.join("release-redelivery"))
                } else {
                    None
                };
                if let Some(release) = wait_marker {
                    let deadline = Instant::now() + Duration::from_secs(60);
                    loop {
                        if release.is_file() {
                            break;
                        }
                        assert!(
                            Instant::now() < deadline,
                            "release marker {} never appeared",
                            release.display()
                        );
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                }
                self.write_durable_effect(&invocation.input).await?;
                Ok(json!({ "committed": true, "cursor": 42 }))
            }
        }
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

async fn wait_for_marker(probe: &mut tokio::process::Child, marker: &Path) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if marker.is_file() {
            return;
        }
        if let Some(status) = probe.try_wait().unwrap() {
            panic!("activity-boundary probe exited with {status} before publishing its marker");
        }
        assert!(
            Instant::now() < deadline,
            "activity-boundary probe did not publish its marker within 60 seconds"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
#[ignore = "private subprocess used only by the PostgreSQL activity-start process-death gate"]
async fn postgres_activity_start_death_probe() {
    run_activity_boundary_probe().await;
}

#[tokio::test]
#[ignore = "private subprocess used only by the PostgreSQL activity-heartbeat process-death gate"]
async fn postgres_activity_heartbeat_death_probe() {
    run_activity_boundary_probe().await;
}

#[tokio::test]
#[ignore = "private subprocess used only by the PostgreSQL activity-unknown process-death gate"]
async fn postgres_activity_unknown_death_probe() {
    run_activity_boundary_probe().await;
}

#[tokio::test]
#[ignore = "private subprocess used only by the PostgreSQL heartbeat-then-completion process-death gate"]
async fn postgres_activity_heartbeat_then_completion_death_probe() {
    run_activity_boundary_probe().await;
}

async fn run_activity_boundary_probe() {
    assert_eq!(std::env::var(PROBE_PARENT_ENV).as_deref(), Ok("1"));
    let postgres_url = std::env::var(PROBE_POSTGRES_ENV).unwrap();
    let queue_name = std::env::var(PROBE_QUEUE_ENV).unwrap();
    let run_id = std::env::var(PROBE_RUN_ENV).unwrap();
    let state_dir = PathBuf::from(std::env::var(PROBE_STATE_ENV).unwrap());
    let marker = PathBuf::from(std::env::var(PROBE_MARKER_ENV).unwrap());
    let boundary = CrashBoundary::from_env();
    let queue = PostgresFlowTaskQueue::connect_with_queue(&postgres_url, queue_name)
        .await
        .unwrap();
    let lease = queue.lease().await.unwrap().expect("probe task lease");
    let store = PostgresEventStore::connect(&postgres_url).await.unwrap();
    let paused = Arc::new(PauseBeforeBoundaryStore {
        inner: store,
        boundary,
        marker: marker.clone(),
        lease_id: lease.lease_id.clone(),
    });
    let runtime = Arc::new(BoundaryRecoveryRuntime::new(&state_dir, boundary));
    let engine = FlowEngine::new(paused.clone(), runtime);
    let worker = FlowWorker::new(engine.clone(), Arc::new(queue));

    match boundary {
        CrashBoundary::Heartbeat => {
            let task = lease.task.clone();
            let drive = tokio::spawn(async move { worker.handle(task).await });
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                let snapshot = engine.snapshot(&run_id).await.unwrap();
                if snapshot
                    .activities
                    .get("durable-effect")
                    .is_some_and(|activity| activity.status == ActivityStatus::Running)
                {
                    let activity = snapshot.activities.get("durable-effect").unwrap();
                    let _ = engine
                        .heartbeat_activity(
                            &run_id,
                            "durable-effect",
                            activity.attempt,
                            &activity.attempt_id,
                            &activity.fencing_token,
                            Some(json!({ "cursor": 7 })),
                        )
                        .await;
                    panic!("heartbeat append returned before process death for run {run_id}");
                }
                if drive.is_finished() {
                    let outcome = drive.await.unwrap();
                    panic!("drive finished before heartbeat boundary: {outcome:?}");
                }
                assert!(
                    Instant::now() < deadline,
                    "activity did not become Running before heartbeat probe deadline"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
        CrashBoundary::CompletionAfterHeartbeat => {
            let task = lease.task.clone();
            let drive = tokio::spawn(async move { worker.handle(task).await });
            let deadline = Instant::now() + Duration::from_secs(60);
            let mut heartbeated = false;
            loop {
                let snapshot = engine.snapshot(&run_id).await.unwrap();
                if !heartbeated
                    && snapshot
                        .activities
                        .get("durable-effect")
                        .is_some_and(|activity| activity.status == ActivityStatus::Running)
                {
                    let activity = snapshot.activities.get("durable-effect").unwrap();
                    engine
                        .heartbeat_activity(
                            &run_id,
                            "durable-effect",
                            activity.attempt,
                            &activity.attempt_id,
                            &activity.fencing_token,
                            Some(json!({ "cursor": 42 })),
                        )
                        .await
                        .expect("heartbeat checkpoint must become durable before completion crash");
                    assert_eq!(
                        engine.snapshot(&run_id).await.unwrap().activities["durable-effect"]
                            .checkpoint,
                        Some(json!({ "cursor": 42 }))
                    );
                    tokio::fs::write(state_dir.join("release-after-heartbeat"), b"1")
                        .await
                        .unwrap();
                    heartbeated = true;
                }
                if drive.is_finished() {
                    let outcome = drive.await.unwrap();
                    panic!(
                        "drive finished before completion-after-heartbeat boundary: {outcome:?}"
                    );
                }
                assert!(
                    Instant::now() < deadline,
                    "completion-after-heartbeat probe did not reach its crash boundary"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
        CrashBoundary::Started | CrashBoundary::Unknown => {
            worker.handle(lease.task).await.unwrap();
            panic!("activity-boundary probe returned before being killed for run {run_id}");
        }
    }
}

#[tokio::test]
async fn postgres_worker_recovers_after_activity_start_process_death() {
    run_activity_boundary_recovery(CrashBoundary::Started).await;
}

#[tokio::test]
async fn postgres_worker_recovers_after_activity_heartbeat_process_death() {
    run_activity_boundary_recovery(CrashBoundary::Heartbeat).await;
}

#[tokio::test]
async fn postgres_worker_recovers_after_activity_unknown_process_death() {
    run_activity_boundary_recovery(CrashBoundary::Unknown).await;
}

#[tokio::test]
async fn postgres_worker_recovers_after_activity_heartbeat_then_completion_process_death() {
    run_activity_boundary_recovery(CrashBoundary::CompletionAfterHeartbeat).await;
}

async fn run_activity_boundary_recovery(boundary: CrashBoundary) {
    let Some(postgres_url) = postgres_url_from_env() else {
        eprintln!(
            "skipping PostgreSQL activity {} process-death test; set A3S_FLOW_POSTGRES_URL",
            boundary.as_env()
        );
        return;
    };
    let scope = Uuid::new_v4();
    let queue_name = format!("activity-boundary-{}-{scope}", boundary.as_env());
    let run_id = format!("activity-boundary-{}-{scope}", boundary.as_env());
    let state = tempfile::tempdir().unwrap();
    let marker = state
        .path()
        .join(format!("{}-boundary.json", boundary.as_env()));
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
        .arg(boundary.probe_test())
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
        .env(PROBE_BOUNDARY_ENV, boundary.as_env())
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
    let mut pre_crash_fence = None;
    let mut pre_crash_attempt_id = None;
    match boundary {
        CrashBoundary::Started => {
            assert!(interrupted_history.iter().any(|event| {
                matches!(event.event, FlowEvent::ActivityCreated { ref activity_id, .. } if activity_id == "durable-effect")
            }));
            assert!(!interrupted_history.iter().any(|event| {
                matches!(event.event, FlowEvent::ActivityStarted { ref activity_id, .. } if activity_id == "durable-effect")
            }));
        }
        CrashBoundary::Heartbeat => {
            assert!(interrupted_history.iter().any(|event| {
                matches!(event.event, FlowEvent::ActivityStarted { ref activity_id, .. } if activity_id == "durable-effect")
            }));
            assert!(!interrupted_history.iter().any(|event| {
                matches!(event.event, FlowEvent::ActivityHeartbeat { ref activity_id, .. } if activity_id == "durable-effect")
            }));
            assert!(!interrupted_history.iter().any(|event| {
                matches!(event.event, FlowEvent::ActivityCompleted { ref activity_id, .. } if activity_id == "durable-effect")
            }));
        }
        CrashBoundary::Unknown => {
            assert!(interrupted_history.iter().any(|event| {
                matches!(event.event, FlowEvent::ActivityStarted { ref activity_id, .. } if activity_id == "durable-effect")
            }));
            assert!(!interrupted_history.iter().any(|event| {
                matches!(event.event, FlowEvent::ActivityUnknown { ref activity_id, .. } if activity_id == "durable-effect")
            }));
            assert!(!interrupted_history.iter().any(|event| {
                matches!(event.event, FlowEvent::ActivityCompleted { ref activity_id, .. } if activity_id == "durable-effect")
            }));
        }
        CrashBoundary::CompletionAfterHeartbeat => {
            assert!(interrupted_history.iter().any(|event| {
                matches!(event.event, FlowEvent::ActivityStarted { ref activity_id, .. } if activity_id == "durable-effect")
            }));
            assert!(interrupted_history.iter().any(|event| {
                matches!(event.event, FlowEvent::ActivityHeartbeat { ref activity_id, .. } if activity_id == "durable-effect")
            }));
            assert!(!interrupted_history.iter().any(|event| {
                matches!(event.event, FlowEvent::ActivityCompleted { ref activity_id, .. } if activity_id == "durable-effect")
            }));
            let interrupted = FlowEngine::new(
                Arc::new(PostgresEventStore::connect(&postgres_url).await.unwrap()),
                Arc::new(BoundaryRecoveryRuntime::new(state.path(), boundary)),
            )
            .snapshot(&run_id)
            .await
            .unwrap();
            assert_eq!(
                interrupted.activities["durable-effect"].status,
                ActivityStatus::Running
            );
            assert_eq!(
                interrupted.activities["durable-effect"].checkpoint,
                Some(json!({ "cursor": 42 })),
                "durable heartbeat checkpoint must survive the completion crash window"
            );
            pre_crash_fence = Some(
                interrupted.activities["durable-effect"]
                    .fencing_token
                    .clone(),
            );
            pre_crash_attempt_id =
                Some(interrupted.activities["durable-effect"].attempt_id.clone());
        }
    }

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
        Arc::new(BoundaryRecoveryRuntime::new(state.path(), boundary)),
    );
    let replacement = FlowWorker::new(replacement_engine.clone(), reconnected_queue.clone());
    if boundary == CrashBoundary::CompletionAfterHeartbeat {
        let pre_crash_fence = pre_crash_fence.expect("pre-crash fencing token");
        let attempt_id = pre_crash_attempt_id.expect("pre-crash attempt id");
        let drive = tokio::spawn(async move { replacement.run_once().await });
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let history = reconnected_store.list(&run_id).await.unwrap();
            if history.iter().any(|event| {
                matches!(
                    event.event,
                    FlowEvent::ActivityLeaseAcquired {
                        ref activity_id,
                        ..
                    } if activity_id == "durable-effect"
                )
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "replacement did not rotate the activity lease before deadline"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let leased = replacement_engine.snapshot(&run_id).await.unwrap();
        assert_eq!(
            leased.activities["durable-effect"].checkpoint,
            Some(json!({ "cursor": 42 }))
        );
        assert_ne!(
            leased.activities["durable-effect"].fencing_token,
            pre_crash_fence
        );
        let stale = replacement_engine
            .heartbeat_activity(
                &run_id,
                "durable-effect",
                1,
                &attempt_id,
                &pre_crash_fence,
                Some(json!({ "cursor": 99 })),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(stale, FlowError::InvalidTransition(ref message) if message.contains("stale")),
            "{stale:?}"
        );
        tokio::fs::write(state.path().join("release-redelivery"), b"1")
            .await
            .unwrap();
        drive.await.unwrap().unwrap().expect("replayed task");
    } else {
        replacement
            .run_once()
            .await
            .unwrap()
            .expect("replayed task");
    }

    let snapshot = replacement_engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(reconnected_queue.len().await.unwrap(), 0);
    assert_eq!(reconnected_queue.inflight_len().await.unwrap(), 0);
    let recovered_history = reconnected_store.list(&run_id).await.unwrap();
    assert_eq!(
        recovered_history
            .iter()
            .filter(|event| {
                matches!(event.event, FlowEvent::ActivityStarted { ref activity_id, .. } if activity_id == "durable-effect")
            })
            .count(),
        1,
        "start must be recorded exactly once after the crash window"
    );
    assert_eq!(
        recovered_history
            .iter()
            .filter(|event| {
                matches!(event.event, FlowEvent::ActivityCompleted { ref activity_id, .. } if activity_id == "durable-effect")
            })
            .count(),
        1
    );
    match boundary {
        CrashBoundary::Started => {
            assert_eq!(
                recovered_history
                    .iter()
                    .filter(|event| {
                        matches!(event.event, FlowEvent::ActivityCreated { ref activity_id, .. } if activity_id == "durable-effect")
                    })
                    .count(),
                1
            );
            assert_eq!(
                recovered_history
                    .iter()
                    .filter(|event| {
                        matches!(event.event, FlowEvent::ActivityLeaseAcquired { ref activity_id, .. } if activity_id == "durable-effect")
                    })
                    .count(),
                0,
                "create→start death must start the pending attempt, not rotate a running lease"
            );
            assert_eq!(
                tokio::fs::read_to_string(state.path().join("physical-attempts.txt"))
                    .await
                    .unwrap()
                    .lines()
                    .filter(|line| !line.is_empty())
                    .count(),
                1,
                "host side effect must run only after start becomes durable"
            );
        }
        CrashBoundary::Heartbeat => {
            // A lost heartbeat append invents no durable heartbeat; recovery may
            // complete without requiring a later heartbeat event.
            assert!(!recovered_history.iter().any(|event| {
                matches!(event.event, FlowEvent::ActivityHeartbeat { ref activity_id, .. } if activity_id == "durable-effect")
            }));
            assert_eq!(
                recovered_history
                    .iter()
                    .filter(|event| {
                        matches!(event.event, FlowEvent::ActivityLeaseAcquired { ref activity_id, .. } if activity_id == "durable-effect")
                    })
                    .count(),
                1,
                "replacement ownership must rotate the fencing token for the same attempt"
            );
            assert_eq!(
                tokio::fs::read_to_string(state.path().join("physical-attempts.txt"))
                    .await
                    .unwrap()
                    .lines()
                    .filter(|line| !line.is_empty())
                    .count(),
                2
            );
        }
        CrashBoundary::Unknown => {
            assert_eq!(
                recovered_history
                    .iter()
                    .filter(|event| {
                        matches!(event.event, FlowEvent::ActivityUnknown { ref activity_id, .. } if activity_id == "durable-effect")
                    })
                    .count(),
                0,
                "undurable unknown outcomes must not leave a suspended Unknown state"
            );
            assert_eq!(
                recovered_history
                    .iter()
                    .filter(|event| {
                        matches!(event.event, FlowEvent::ActivityLeaseAcquired { ref activity_id, .. } if activity_id == "durable-effect")
                    })
                    .count(),
                1,
                "replacement ownership must rotate the fencing token for the same attempt"
            );
            assert_eq!(
                tokio::fs::read_to_string(state.path().join("physical-attempts.txt"))
                    .await
                    .unwrap()
                    .lines()
                    .filter(|line| !line.is_empty())
                    .count(),
                2
            );
        }
        CrashBoundary::CompletionAfterHeartbeat => {
            assert_eq!(
                recovered_history
                    .iter()
                    .filter(|event| {
                        matches!(event.event, FlowEvent::ActivityHeartbeat { ref activity_id, .. } if activity_id == "durable-effect")
                    })
                    .count(),
                1
            );
            assert_eq!(
                recovered_history
                    .iter()
                    .filter(|event| {
                        matches!(event.event, FlowEvent::ActivityLeaseAcquired { ref activity_id, .. } if activity_id == "durable-effect")
                    })
                    .count(),
                1,
                "replacement ownership must rotate the fencing token for the same attempt"
            );
            assert_eq!(
                snapshot.activities["durable-effect"].checkpoint,
                Some(json!({ "cursor": 42 })),
                "durable heartbeat checkpoint must remain visible after replacement completion"
            );
            assert_eq!(snapshot.activities["durable-effect"].attempt, 1);
            assert_eq!(
                tokio::fs::read_to_string(state.path().join("physical-attempts.txt"))
                    .await
                    .unwrap()
                    .lines()
                    .filter(|line| !line.is_empty())
                    .count(),
                2
            );
        }
    }
    assert_eq!(
        tokio::fs::read_to_string(state.path().join("logical-effect.txt"))
            .await
            .unwrap(),
        format!("{run_id}:durable-effect")
    );
}

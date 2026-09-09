//! FLOW-R6 certification harness: frozen protocol fixtures, chaos boundaries,
//! and bounded load correctness for the durable execution kernel.

use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, FlowRuntime, FlowTask,
    FlowTaskQueue, FlowWorkerCapabilities, InMemoryEventStore, InMemoryFlowTaskQueue,
    LocalFileFlowTaskQueue, RetryPolicy, RuntimeCommand, WorkflowInvocation, WorkflowSpec,
    FLOW_EVENT_ENVELOPE_SCHEMA_VERSION, FLOW_WORKER_PROTOCOL,
};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

struct CertRuntime;

#[async_trait]
impl FlowRuntime for CertRuntime {
    async fn run_workflow(
        &self,
        _invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Err(FlowError::Runtime(
            "certification runtime is not executable".to_string(),
        ))
    }

    async fn run_step(
        &self,
        _invocation: a3s_flow::StepInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime(
            "certification runtime is not executable".to_string(),
        ))
    }
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/protocol")
        .join(name)
}

fn read_fixture(name: &str) -> serde_json::Value {
    let bytes = std::fs::read(fixture(name)).unwrap_or_else(|error| {
        panic!("missing certification fixture {name}: {error}");
    });
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!("invalid certification fixture {name}: {error}");
    })
}

#[test]
fn protocol_fixtures_match_current_worker_and_task_wire_shapes() {
    let caps: FlowWorkerCapabilities =
        serde_json::from_value(read_fixture("worker_capabilities.v1.json")).unwrap();
    assert_eq!(caps, FlowWorkerCapabilities::current());
    assert_eq!(caps.protocol, FLOW_WORKER_PROTOCOL);
    FlowWorkerCapabilities::negotiate(&caps, &FlowWorkerCapabilities::current()).unwrap();

    let task: FlowTask =
        serde_json::from_value(read_fixture("flow_task.drive_run.v1.json")).unwrap();
    assert_eq!(
        task,
        FlowTask::DriveRun {
            run_id: "cert-run".to_string()
        }
    );
    assert_eq!(
        serde_json::to_value(&task).unwrap(),
        read_fixture("flow_task.drive_run.v1.json")
    );
}

#[test]
fn protocol_fixtures_preserve_event_envelope_schema_version() {
    let envelope: FlowEventEnvelope =
        serde_json::from_value(read_fixture("event_envelope.run_created.v1.json")).unwrap();
    assert_eq!(envelope.schema_version, FLOW_EVENT_ENVELOPE_SCHEMA_VERSION);
    envelope.validate_schema_version().unwrap();
    assert_eq!(envelope.run_id, "cert-run");
    assert_eq!(envelope.sequence, 1);
    assert!(matches!(
        envelope.event,
        FlowEvent::RunCreated { ref spec, .. } if spec.name == "cert.protocol"
    ));
    assert_eq!(
        serde_json::to_value(&envelope).unwrap(),
        read_fixture("event_envelope.run_created.v1.json")
    );
}

#[test]
fn protocol_fixtures_preserve_visibility_projection_wire_shape() {
    let projection: a3s_flow::FlowVisibilityProjection =
        serde_json::from_value(read_fixture("visibility_projection.v1.json")).unwrap();
    projection.validate().unwrap();
    assert_eq!(
        projection.schema_version,
        a3s_flow::FLOW_VISIBILITY_PROJECTION_SCHEMA_VERSION
    );
    assert_eq!(projection.run_id, "cert-vis");
    assert_eq!(projection.workflow_name, "cert.visibility");
    assert!(serde_json::to_value(&projection)
        .unwrap()
        .get("input")
        .is_none());
    assert_eq!(
        serde_json::to_value(&projection).unwrap(),
        read_fixture("visibility_projection.v1.json")
    );
}

#[tokio::test]
async fn chaos_stale_queue_lease_cannot_ack_after_rotation() {
    let queue = InMemoryFlowTaskQueue::new();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "chaos-lease".to_string(),
        })
        .await
        .unwrap();
    let lease = queue.lease().await.unwrap().unwrap();
    let renewed = queue.heartbeat(&lease.lease_id).await.unwrap();
    let stale = queue.ack(&lease.lease_id).await.unwrap_err();
    assert!(matches!(stale, FlowError::LeaseLost(id) if id == lease.lease_id));
    queue.ack(&renewed).await.unwrap();
}

#[tokio::test]
async fn chaos_local_file_queue_rejects_forged_lease_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let queue = LocalFileFlowTaskQueue::new(dir.path());
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "chaos-forge".to_string(),
        })
        .await
        .unwrap();
    let _lease = queue.lease().await.unwrap().unwrap();
    let forged = queue.ack("not-a-lease.json").await.unwrap_err();
    assert!(matches!(forged, FlowError::LeaseLost(_)));
}

#[tokio::test]
async fn load_tip_pinned_archive_export_stays_correct_for_ten_thousand_events() {
    let store = Arc::new(InMemoryEventStore::new());
    let run_id = "cert-load-10k";
    store
        .append(
            run_id,
            FlowEvent::RunCreated {
                spec: WorkflowSpec::rust_embedded("cert.load", "1", "tests::certification", "main"),
                input: json!({ "n": 10_000 }),
            },
        )
        .await
        .unwrap();
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
    for index in 0..9_998 {
        store
            .append(
                run_id,
                FlowEvent::WaitCreated {
                    wait_id: format!("w-{index}"),
                    resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
                },
            )
            .await
            .unwrap();
    }

    let engine = FlowEngine::new(Arc::clone(&store) as _, Arc::new(CertRuntime));
    let started = Instant::now();
    let seal = engine
        .export_history_archive(run_id, 1_000, |_page| async { Ok(()) })
        .await
        .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(seal.event_count, 10_000);
    engine.verify_history_archive_seal(&seal).await.unwrap();
    // Engineering gate: the tip-pinned export must finish promptly on the
    // in-memory adapter without materializing a second authority.
    assert!(
        elapsed.as_secs() < 30,
        "archive export of 10_000 events took {elapsed:?}"
    );
}

#[tokio::test]
async fn chaos_mixed_worker_protocol_negotiation_fails_closed() {
    let engine = FlowEngine::in_memory(Arc::new(CertRuntime));
    let worker = a3s_flow::FlowWorker::in_memory(engine);
    let mut required = worker.capabilities();
    required.protocol = "a3s.flow.worker.v0".to_string();
    let error = worker.ensure_compatible(&required).unwrap_err();
    assert!(matches!(error, FlowError::UnsupportedWorkerProtocol { .. }));
}

#[tokio::test]
async fn chaos_stale_activity_fencing_token_cannot_complete() {
    let store = InMemoryEventStore::new();
    let run_id = "chaos-activity-fence";
    store
        .append(
            run_id,
            FlowEvent::RunCreated {
                spec: WorkflowSpec::rust_embedded(
                    "cert.chaos.activity",
                    "1",
                    "tests::certification",
                    "main",
                ),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
    store
        .append(
            run_id,
            FlowEvent::ActivityCreated {
                activity_id: "send".to_string(),
                activity_name: "send".to_string(),
                input: json!({}),
                retry: RetryPolicy::default(),
                timeout_ms: None,
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::ActivityStarted {
                activity_id: "send".to_string(),
                attempt: 1,
                attempt_id: "attempt-1".to_string(),
                idempotency_key: "key-1".to_string(),
                fencing_token: "fence-1".to_string(),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::ActivityLeaseAcquired {
                activity_id: "send".to_string(),
                attempt: 1,
                attempt_id: "attempt-1".to_string(),
                fencing_token: "fence-2".to_string(),
            },
        )
        .await
        .unwrap();
    let error = store
        .append(
            run_id,
            FlowEvent::ActivityCompleted {
                activity_id: "send".to_string(),
                attempt_id: "attempt-1".to_string(),
                fencing_token: "fence-1".to_string(),
                output: json!(true),
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(&error, FlowError::InvalidTransition(message) if message.contains("fencing identity does not match")),
        "expected stale fencing rejection, got {error:?}"
    );
}

#[test]
fn upgrade_retained_pre_v1_histories_deserialize_without_synthesizing_fields() {
    for (name, source) in [
        (
            "v0.5.0",
            include_str!("fixtures/pre_v1/v0.5.0-running-step.json"),
        ),
        (
            "v0.13.1",
            include_str!("fixtures/pre_v1/v0.13.1-running-step.json"),
        ),
    ] {
        let json: serde_json::Value =
            serde_json::from_str(source).unwrap_or_else(|error| panic!("{name}: {error}"));
        let history: Vec<FlowEventEnvelope> = serde_json::from_value(json.clone())
            .unwrap_or_else(|error| panic!("{name} deserialize: {error}"));
        assert!(
            !history.is_empty(),
            "{name} fixture must contain retained events"
        );
        assert_eq!(
            serde_json::to_value(&history).unwrap(),
            json,
            "{name} must round-trip without synthesizing durable fields"
        );
        for envelope in &history {
            envelope
                .validate_schema_version()
                .unwrap_or_else(|error| panic!("{name} schema: {error}"));
        }
    }
}

use super::*;

struct BlockingObserver;

#[async_trait]
impl FlowEventObserver for BlockingObserver {
    async fn observe(&self, _envelope: FlowEventEnvelope) {
        pending::<()>().await;
    }
}

struct BarrierObserver {
    barrier: Arc<Barrier>,
}

#[async_trait]
impl FlowEventObserver for BarrierObserver {
    async fn observe(&self, _envelope: FlowEventEnvelope) {
        self.barrier.wait().await;
    }
}

fn audit_event(run_id: &str, sequence: u64) -> A3sFlowEvent {
    A3sFlowEvent::from_envelope(
        &FlowEventEnvelope::new(
            run_id,
            sequence,
            Uuid::new_v4(),
            fixed_time(),
            FlowEvent::RunStarted,
        ),
        None,
    )
}

#[test]
fn bridged_events_preserve_attempt_correlation_without_metric_cardinality() {
    let bridged = A3sFlowEvent::from_envelope(
        &envelope(
            "attempt-run",
            4,
            FlowEvent::ActivityStarted {
                activity_id: "activity-1".to_string(),
                attempt: 2,
                attempt_id: "attempt-2".to_string(),
                idempotency_key: "activity-1:attempt-2".to_string(),
                fencing_token: "fence-2".to_string(),
            },
        ),
        None,
    );

    assert_eq!(bridged.attempt, Some(2));
    assert_eq!(bridged.attempt_id.as_deref(), Some("attempt-2"));
    assert_eq!(
        bridged.idempotency_key.as_deref(),
        Some("activity-1:attempt-2")
    );
    let labels = bridged.safe_metric_labels();
    assert!(!labels.contains_key("attempt"));
    assert!(!labels.contains_key("attempt_id"));
    assert!(!labels.contains_key("idempotency_key"));

    let encoded = serde_json::to_value(bridged).unwrap();
    assert_eq!(encoded["attempt"], 2);
    assert_eq!(encoded["attempt_id"], "attempt-2");
    assert_eq!(encoded["idempotency_key"], "activity-1:attempt-2");
}

#[tokio::test]
async fn observer_receives_committed_events_in_store_order() {
    let observer = Arc::new(InMemoryFlowEventObserver::new());
    let engine = FlowEngine::builder(Arc::new(SequentialRuntime))
        .with_observer(observer.clone())
        .build();

    let run_id = engine
        .start_with_id("observed-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    let stored = engine.store().list(&run_id).await.unwrap();
    let observed = observer.events().await;

    assert_eq!(observed, stored);
    assert_eq!(
        observer.event_keys().await,
        stored
            .iter()
            .map(|event| event.event.event_key())
            .collect::<Vec<_>>()
    );

    engine
        .start_with_id("observed-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    assert_eq!(
        observer.events().await.len(),
        stored.len(),
        "idempotent start should not append or observe duplicate events"
    );
}

#[tokio::test]
async fn fanout_observer_forwards_committed_events_to_each_observer() {
    let raw_observer = Arc::new(InMemoryFlowEventObserver::new());
    let a3s_sink = Arc::new(InMemoryA3sFlowEventSink::new());
    let bridge = Arc::new(A3sFlowEventBridge::new(a3s_sink.clone()));
    let fanout = Arc::new(
        FanoutFlowEventObserver::new()
            .with_observer(raw_observer.clone())
            .with_observer(bridge),
    );
    assert_eq!(fanout.len(), 2);

    let engine = FlowEngine::builder(Arc::new(SequentialRuntime))
        .with_observer(fanout)
        .build();
    let run_id = engine
        .start_with_id("fanout-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();

    let stored = engine.store().list(&run_id).await.unwrap();
    let raw_events = raw_observer.events().await;
    let a3s_events = a3s_sink.events().await;

    assert_eq!(raw_events, stored);
    assert_eq!(a3s_events.len(), stored.len());
    assert_eq!(a3s_events.first().unwrap().key, "flow.run.created");
    assert_eq!(a3s_events.last().unwrap().key, "flow.run.completed");
    assert!(a3s_events.iter().all(|event| event.run_id == "fanout-run"));
}

#[tokio::test]
async fn fanout_observer_delivers_to_downstreams_concurrently() {
    let barrier = Arc::new(Barrier::new(2));
    let fanout = FanoutFlowEventObserver::new()
        .with_observer(Arc::new(BarrierObserver {
            barrier: barrier.clone(),
        }))
        .with_observer(Arc::new(BarrierObserver {
            barrier: barrier.clone(),
        }));

    tokio::time::timeout(
        Duration::from_millis(250),
        fanout.observe(envelope("fanout-concurrent", 1, FlowEvent::RunStarted)),
    )
    .await
    .expect("fanout must not serialize downstream observers");
}

#[tokio::test]
async fn fanout_observer_keeps_healthy_downstreams_after_a_panic() {
    let healthy = Arc::new(InMemoryFlowEventObserver::new());
    let fanout = FanoutFlowEventObserver::new()
        .with_observer(Arc::new(PanickingObserver))
        .with_observer(healthy.clone());

    fanout
        .observe(envelope("fanout-panic", 1, FlowEvent::RunStarted))
        .await;

    assert_eq!(healthy.events().await.len(), 1);
}

#[tokio::test]
async fn observer_timeout_keeps_durable_transition_live() {
    let engine = FlowEngine::builder(Arc::new(SequentialRuntime))
        .with_observer(Arc::new(BlockingObserver))
        .with_observer_timeout(Duration::from_millis(10))
        .build();
    let started = std::time::Instant::now();

    let run_id = engine
        .start_with_id("observer-timeout", spec(), json!({}))
        .await
        .unwrap();

    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(
        engine.snapshot(&run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(engine.observer_timeout(), Duration::from_millis(10));
}

struct PanickingObserver;

#[async_trait]
impl FlowEventObserver for PanickingObserver {
    async fn observe(&self, _envelope: FlowEventEnvelope) {
        panic!("observer failure must not change durable state");
    }
}

#[tokio::test]
async fn observer_panics_are_isolated_from_durable_transition() {
    let engine = FlowEngine::builder(Arc::new(SequentialRuntime))
        .with_observer(Arc::new(PanickingObserver))
        .with_observer_timeout(Duration::from_millis(100))
        .build();

    let run_id = engine
        .start_with_id("observer-panic", spec(), json!({}))
        .await
        .unwrap();
    assert_eq!(
        engine.snapshot(&run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
}

#[tokio::test]
async fn a3s_event_bridge_maps_committed_events_to_safe_labels() {
    let sink = Arc::new(InMemoryA3sFlowEventSink::new());
    let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
    let engine = FlowEngine::builder(Arc::new(SequentialRuntime))
        .with_observer(observer.clone())
        .build();

    engine
        .start_with_id("bridge-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();

    let events = sink.events().await;
    assert_eq!(events.first().unwrap().key, "flow.run.created");
    assert_eq!(
        events
            .iter()
            .map(|event| event.key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "flow.run.created",
            "flow.run.started",
            "flow.step.created",
            "flow.step.started",
            "flow.step.completed",
            "flow.step.created",
            "flow.step.started",
            "flow.step.completed",
            "flow.run.completed",
        ]
    );
    assert!(events.iter().all(|event| event
        .workflow
        .as_ref()
        .is_some_and(|workflow| workflow.name == "test.workflow" && workflow.version == "0.1.0")));

    let step_completed = events
        .iter()
        .find(|event| event.key == "flow.step.completed")
        .unwrap();
    assert_eq!(step_completed.status.as_deref(), Some("completed"));
    assert_eq!(step_completed.subject.as_ref().unwrap().kind, "step");
    assert_eq!(step_completed.subject.as_ref().unwrap().id, "load-user");

    let labels = step_completed.safe_metric_labels();
    assert_eq!(labels["event_key"], "flow.step.completed");
    assert_eq!(labels["workflow_name"], "test.workflow");
    assert_eq!(labels["workflow_version"], "0.1.0");
    assert_eq!(labels["status"], "completed");
    assert!(!labels.contains_key("run_id"));
    assert_eq!(observer.cached_workflow_count().await, 0);
}

#[cfg(feature = "a3s-event")]
#[tokio::test]
async fn a3s_event_bus_sink_publishes_committed_events() {
    let bus = Arc::new(a3s_event::EventBus::new(
        a3s_event::MemoryProvider::default(),
    ));
    let sink = Arc::new(A3sEventBusFlowEventSink::new(bus.clone()));
    let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
    let engine = FlowEngine::builder(Arc::new(SequentialRuntime))
        .with_observer(observer)
        .build();

    engine
        .start_with_id("a3s-event-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();

    let events = bus.list_events(Some("flow"), 20).await.unwrap();
    assert_eq!(events.len(), 9);
    assert!(sink.last_error().await.is_none());

    let run_created = events
        .iter()
        .find(|event| event.event_type == "flow.run.created")
        .unwrap();
    assert_eq!(run_created.subject, "events.flow.run.created");
    assert_eq!(run_created.category, "flow");
    assert_eq!(run_created.source, "a3s-flow");
    assert_eq!(run_created.metadata["flow.run_id"], "a3s-event-run");
    assert_eq!(run_created.metadata["flow.workflow_name"], "test.workflow");
    assert_eq!(run_created.metadata["flow.workflow_version"], "0.1.0");

    let step_completed = events
        .iter()
        .find(|event| {
            event.event_type == "flow.step.completed"
                && event
                    .metadata
                    .get("flow.subject_id")
                    .is_some_and(|subject_id| subject_id == "load-user")
        })
        .unwrap();
    assert_eq!(step_completed.subject, "events.flow.step.completed");
    assert_eq!(step_completed.metadata["flow.status"], "completed");
    assert_eq!(step_completed.metadata["flow.subject_kind"], "step");
    assert_eq!(step_completed.metadata["flow.subject_id"], "load-user");
    assert_eq!(step_completed.payload["run_id"], "a3s-event-run");
    assert_eq!(step_completed.payload["key"], "flow.step.completed");
}

#[tokio::test]
async fn local_file_a3s_event_sink_persists_jsonl_audit_events() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit/flow-events.jsonl");
    let sink = Arc::new(LocalFileA3sFlowEventSink::new(&path));
    let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
    let engine = FlowEngine::builder(Arc::new(SequentialRuntime))
        .with_observer(observer)
        .build();

    engine
        .start_with_id("audit-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();

    let events = sink.events().await.unwrap();
    assert_eq!(events.len(), 9);
    assert_eq!(events.first().unwrap().key, "flow.run.created");
    assert_eq!(events.last().unwrap().key, "flow.run.completed");
    assert!(events.iter().all(|event| event.run_id == "audit-run"));
    assert!(sink.last_error().await.is_none());
    assert_eq!(sink.path(), path.as_path());

    let raw = tokio::fs::read_to_string(&path).await.unwrap();
    assert_eq!(raw.lines().count(), events.len());
    assert!(raw.contains(r#""key":"flow.step.completed""#));

    engine
        .start_with_id("audit-run", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    assert_eq!(
        sink.events().await.unwrap().len(),
        events.len(),
        "idempotent start should not append duplicate audit events"
    );
}

#[tokio::test]
async fn local_file_a3s_event_sink_preserves_an_unterminated_complete_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow-events.jsonl");
    let first = audit_event("audit-first", 1);
    let second = audit_event("audit-second", 2);
    tokio::fs::write(&path, serde_json::to_vec(&first).unwrap())
        .await
        .unwrap();

    let sink = LocalFileA3sFlowEventSink::new(&path);
    sink.emit(second.clone()).await;

    assert!(sink.last_error().await.is_none());
    assert_eq!(sink.events().await.unwrap(), vec![first, second]);
    let repaired = tokio::fs::read(&path).await.unwrap();
    assert!(repaired.ends_with(b"\n"));
    assert_eq!(repaired.split(|byte| *byte == b'\n').count(), 3);
}

#[tokio::test]
async fn local_file_a3s_event_sink_discards_only_an_unterminated_torn_tail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow-events.jsonl");
    let first = audit_event("audit-first", 1);
    let second = audit_event("audit-second", 2);
    let mut bytes = serde_json::to_vec(&first).unwrap();
    bytes.push(b'\n');
    bytes.extend_from_slice(br#"{"torn":"never-complete"#);
    tokio::fs::write(&path, bytes).await.unwrap();

    let sink = LocalFileA3sFlowEventSink::new(&path);
    assert_eq!(sink.events().await.unwrap(), vec![first.clone()]);
    sink.emit(second.clone()).await;

    assert!(sink.last_error().await.is_none());
    assert_eq!(sink.events().await.unwrap(), vec![first, second]);
    let repaired = tokio::fs::read_to_string(&path).await.unwrap();
    assert!(!repaired.contains("never-complete"));
    assert_eq!(repaired.lines().count(), 2);
}

#[tokio::test]
async fn local_file_a3s_event_sink_rejects_terminated_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow-events.jsonl");
    let first = audit_event("audit-first", 1);
    let second = audit_event("audit-second", 2);
    let mut bytes = serde_json::to_vec(&first).unwrap();
    bytes.extend_from_slice(b"\nnot-json\n");
    tokio::fs::write(&path, &bytes).await.unwrap();

    let sink = LocalFileA3sFlowEventSink::new(&path);
    let error = sink.events().await.unwrap_err();
    assert!(error
        .to_string()
        .contains("failed to decode audit event line 2"));

    sink.emit(second).await;

    assert!(sink
        .last_error()
        .await
        .is_some_and(|error| error.contains("failed to decode audit event line 2")));
    assert_eq!(tokio::fs::read(path).await.unwrap(), bytes);
}

#[tokio::test]
async fn local_file_a3s_event_sink_rejects_interior_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow-events.jsonl");
    let first = audit_event("audit-first", 1);
    let second = audit_event("audit-second", 2);
    let third = audit_event("audit-third", 3);
    let mut bytes = serde_json::to_vec(&first).unwrap();
    bytes.extend_from_slice(b"\nnot-json\n");
    bytes.extend_from_slice(&serde_json::to_vec(&third).unwrap());
    bytes.push(b'\n');
    tokio::fs::write(&path, &bytes).await.unwrap();

    let sink = LocalFileA3sFlowEventSink::new(&path);
    let error = sink.events().await.unwrap_err();
    assert!(error
        .to_string()
        .contains("failed to decode audit event line 2"));

    sink.emit(second).await;

    assert!(sink.last_error().await.is_some());
    assert_eq!(tokio::fs::read(path).await.unwrap(), bytes);
}

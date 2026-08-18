use super::*;

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
async fn a3s_event_bridge_maps_committed_events_to_safe_labels() {
    let sink = Arc::new(InMemoryA3sFlowEventSink::new());
    let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
    let engine = FlowEngine::builder(Arc::new(SequentialRuntime))
        .with_observer(observer)
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

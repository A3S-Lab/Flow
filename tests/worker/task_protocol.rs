use super::*;

#[test]
fn flow_task_serializes_for_external_queues() {
    let now = "2026-08-08T12:34:56.123456789Z"
        .parse::<DateTime<Utc>>()
        .unwrap();
    let task = FlowTask::ResumeScheduledRun {
        run_id: "run-1".to_string(),
        now,
    };

    let encoded = serde_json::to_string(&task).unwrap();
    assert_eq!(
        encoded,
        r#"{"type":"resume_scheduled_run","run_id":"run-1","now":"2026-08-08T12:34:56.123456789Z"}"#
    );

    let decoded: FlowTask = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, task);

    let task = FlowTask::ResumeHookByToken {
        token: "approval-token".to_string(),
        payload: json!({ "approved": true }),
    };

    let encoded = serde_json::to_string(&task).unwrap();
    assert_eq!(
        encoded,
        r#"{"type":"resume_hook_by_token","token":"approval-token","payload":{"approved":true}}"#
    );

    let decoded: FlowTask = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, task);

    let task = FlowTask::DisposeHookByToken {
        token: "approval-token".to_string(),
    };

    let encoded = serde_json::to_string(&task).unwrap();
    assert_eq!(
        encoded,
        r#"{"type":"dispose_hook_by_token","token":"approval-token"}"#
    );

    let decoded: FlowTask = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, task);

    let lease = FlowTaskLease::new("lease-1", task);
    assert_eq!(lease.lease_id, "lease-1");
    assert!(matches!(
        lease.task,
        FlowTask::DisposeHookByToken { token } if token == "approval-token"
    ));
}

#[test]
fn worker_capabilities_negotiate_fail_closed() {
    let required = FlowWorkerCapabilities::current();
    let offered = FlowWorkerCapabilities::current();
    FlowWorkerCapabilities::negotiate(&required, &offered).unwrap();

    let mut incompatible = offered.clone();
    incompatible.protocol = "a3s.flow.worker.v0".to_string();
    let error = FlowWorkerCapabilities::negotiate(&required, &incompatible).unwrap_err();
    assert!(matches!(error, FlowError::UnsupportedWorkerProtocol { .. }));

    let mut missing = offered;
    missing.bounded_drain = false;
    let error = FlowWorkerCapabilities::negotiate(&required, &missing).unwrap_err();
    assert!(
        matches!(error, FlowError::WorkerCapabilityUnavailable(message) if message == "bounded_drain")
    );

    let legacy: FlowWorkerCapabilities = serde_json::from_value(json!({
        "protocol": FLOW_WORKER_PROTOCOL
    }))
    .unwrap();
    let minimal = FlowWorkerCapabilities {
        protocol: FLOW_WORKER_PROTOCOL.to_string(),
        task_types: Vec::new(),
        lease_fencing: false,
        heartbeats: false,
        bounded_drain: false,
    };
    FlowWorkerCapabilities::negotiate(&minimal, &legacy).unwrap();
}

#[test]
fn worker_capabilities_round_trip_and_task_kinds_are_stable() {
    let capabilities = FlowWorkerCapabilities::current();
    let encoded = serde_json::to_value(&capabilities).unwrap();
    assert_eq!(encoded["protocol"], FLOW_WORKER_PROTOCOL);
    assert_eq!(encoded["task_types"][0], "drive_run");
    assert!(capabilities.supports_task("resume_scheduled_run"));
    assert!(!capabilities.supports_task("future_task"));

    let decoded: FlowWorkerCapabilities = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, capabilities);
    assert_eq!(
        FlowTask::DriveRun { run_id: "r".into() }.kind(),
        "drive_run"
    );
}

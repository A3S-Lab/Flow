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

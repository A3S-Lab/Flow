use a3s_flow::{WorkflowRunSnapshot, WorkflowRunStatus, WorkflowSpec};
use serde_json::json;

#[test]
fn public_snapshot_constructor_starts_from_one_empty_pending_projection() {
    let snapshot = WorkflowRunSnapshot::new(
        "run-1",
        WorkflowSpec::rust_embedded("test.workflow", "1", "tests", "run"),
        json!({"input": true}),
    );

    assert_eq!(snapshot.run_id, "run-1");
    assert_eq!(snapshot.status, WorkflowRunStatus::Pending);
    assert_eq!(snapshot.input, json!({"input": true}));
    assert_eq!(snapshot.last_sequence, 0);
    assert!(snapshot.steps.is_empty());
    assert!(snapshot.waits.is_empty());
    assert!(snapshot.hooks.is_empty());
    assert!(snapshot.child_operations.is_empty());
    assert!(snapshot.child_workflows.is_empty());
    assert!(snapshot.signals.is_empty());
    assert!(snapshot.signal_waits.is_empty());
    assert!(snapshot.output.is_none());
    assert!(snapshot.error.is_none());
    assert!(snapshot.terminal_outcome.is_none());
    assert!(snapshot.continuation.is_none());
}

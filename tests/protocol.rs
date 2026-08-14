use a3s_flow::{
    FlowEvent, FlowEventEnvelope, NativeRuntimeKind, NativeRuntimeRequest, NativeRuntimeResponse,
    NativeTsCompilerCapabilities, NativeTsDependencyManifest, RuntimeCommand, WorkflowRunSummary,
    NATIVE_COMPILER_PROTOCOL, NATIVE_DEPENDENCY_MANIFEST_PROTOCOL, NATIVE_RUNTIME_PROTOCOL,
};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

#[test]
fn native_runtime_request_uses_stable_json_field_names() {
    let request = NativeRuntimeRequest::new(
        NativeRuntimeKind::Workflow,
        "main",
        "abc123",
        json!({ "run_id": "run-1" }),
    );

    let encoded = serde_json::to_value(request).unwrap();
    assert_eq!(encoded["protocol"], NATIVE_RUNTIME_PROTOCOL);
    assert_eq!(encoded["kind"], "workflow");
    assert_eq!(encoded["exportName"], "main");
    assert_eq!(encoded["sourceHash"], "abc123");
    assert_eq!(encoded["payload"]["run_id"], "run-1");
}

#[test]
fn native_runtime_response_decodes_kind_and_error_envelope() {
    let response: NativeRuntimeResponse = serde_json::from_value(json!({
        "protocol": NATIVE_RUNTIME_PROTOCOL,
        "kind": "step",
        "ok": false,
        "error": "step failed"
    }))
    .unwrap();

    assert_eq!(response.kind, NativeRuntimeKind::Step);
    assert_eq!(response.error.as_deref(), Some("step failed"));
}

#[test]
fn native_compiler_protocol_has_stable_capability_and_manifest_fields() {
    let capabilities = serde_json::to_value(NativeTsCompilerCapabilities::current()).unwrap();
    assert_eq!(capabilities["protocol"], NATIVE_COMPILER_PROTOCOL);
    assert_eq!(capabilities["dependencyManifest"], true);

    let manifest = serde_json::to_value(NativeTsDependencyManifest::new(
        "bun-sha256:test",
        vec!["src/main.ts".to_string(), "src/shared.ts".to_string()],
    ))
    .unwrap();
    assert_eq!(manifest["protocol"], NATIVE_DEPENDENCY_MANIFEST_PROTOCOL);
    assert_eq!(manifest["compilerIdentity"], "bun-sha256:test");
    assert_eq!(manifest["files"][0], "src/main.ts");
    assert_eq!(manifest["files"][1], "src/shared.ts");
}

#[test]
fn native_runtime_operation_commands_accept_omitted_optional_fields() {
    let progress: RuntimeCommand = serde_json::from_value(json!({
        "type": "record_progress",
        "progress": {
            "progress_id": "download-1",
            "completed": 1
        }
    }))
    .unwrap();
    match progress {
        RuntimeCommand::RecordProgress { progress } => {
            assert_eq!(progress.total, None);
            assert_eq!(progress.message, None);
            assert_eq!(progress.details, serde_json::Value::Null);
        }
        command => panic!("unexpected command: {command:?}"),
    }

    let child: RuntimeCommand = serde_json::from_value(json!({
        "type": "link_child_operation",
        "child": {
            "reference_id": "runtime",
            "kind": "runtime.unit",
            "operation_id": "runtime-1"
        }
    }))
    .unwrap();
    match child {
        RuntimeCommand::LinkChildOperation { child } => {
            assert_eq!(child.flow_run_id, None);
            assert_eq!(child.metadata, serde_json::Value::Null);
        }
        command => panic!("unexpected command: {command:?}"),
    }
}

#[test]
fn run_summary_accepts_payloads_from_before_cancelling_state_was_added() {
    let summary: WorkflowRunSummary = serde_json::from_value(json!({
        "total_runs": 0,
        "pending_runs": 0,
        "running_runs": 0,
        "suspended_runs": 0,
        "completed_runs": 0,
        "failed_runs": 0,
        "cancelled_runs": 0,
        "terminal_runs": 0,
        "non_terminal_runs": 0,
        "open_waits": 0,
        "active_hooks": 0,
        "pending_retries": 0
    }))
    .unwrap();
    assert_eq!(summary.cancelling_runs, 0);
}

#[test]
fn flow_event_envelope_uses_stable_native_history_field_names() {
    let envelope = FlowEventEnvelope {
        run_id: "run-1".to_string(),
        sequence: 7,
        event_id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event: FlowEvent::RunCancelled { reason: None },
    };

    let encoded = serde_json::to_value(envelope).unwrap();
    assert_eq!(encoded["run_id"], "run-1");
    assert_eq!(encoded["sequence"], 7);
    assert!(encoded["event_id"].as_str().is_some());
    assert!(encoded["timestamp"].as_str().is_some());
    assert_eq!(encoded["event"]["type"], "run_cancelled");
    assert_eq!(encoded["event"]["reason"], serde_json::Value::Null);
    assert!(
        encoded.get("key").is_none(),
        "native runtime history envelopes should not include derived event keys"
    );
}

#[test]
fn native_ts_authoring_types_track_runtime_protocol_shape() {
    let types = include_str!("../examples/native-ts/a3s-flow-runtime.d.ts");
    for event_type in [
        "run_created",
        "run_started",
        "run_completed",
        "run_failed",
        "run_cancellation_requested",
        "run_cancelled",
        "run_timed_out",
        "run_retry_exhausted",
        "run_host_shutdown",
        "run_progress_recorded",
        "child_operation_linked",
        "step_created",
        "step_started",
        "step_completed",
        "step_retrying",
        "step_failed",
        "wait_created",
        "wait_completed",
        "hook_created",
        "hook_received",
        "hook_disposed",
    ] {
        assert!(
            types.contains(&format!("\"{event_type}\"")),
            "missing FlowEvent type {event_type} in native TypeScript authoring contract"
        );
    }

    assert!(types.contains("event_id: string;"));
    assert!(!types.contains("key: string;"));
    assert!(types.contains("token: string;"));
    assert!(types.contains("retry?: RetryPolicy;"));
    assert!(types.contains("retry_after: string | null;"));
    assert!(types.contains("type: \"record_progress\"; progress: WorkflowProgress"));
    assert!(types.contains("type: \"link_child_operation\"; child: ChildOperationReference"));
    assert!(types.contains("type: \"cancel\""));
    assert!(!types.contains("export function stepOutput"));
}

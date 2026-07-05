use a3s_flow::{
    FlowEvent, FlowEventEnvelope, NativeRuntimeKind, NativeRuntimeRequest, NativeRuntimeResponse,
    NATIVE_RUNTIME_PROTOCOL,
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
        "run_cancelled",
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
    assert!(!types.contains("export function stepOutput"));
}

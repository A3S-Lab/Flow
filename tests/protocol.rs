use a3s_flow::{
    NativeRuntimeKind, NativeRuntimeRequest, NativeRuntimeResponse, NATIVE_RUNTIME_PROTOCOL,
};
use serde_json::json;

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

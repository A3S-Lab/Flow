use a3s_flow::{
    apply_workflow_authoring_operation, canonical_workflow_authoring_snapshot,
    validate_executable_workflow_authoring_snapshot, WorkflowDslError,
    WORKFLOW_AUTHORING_OPERATION_MAX_BYTES,
};
use serde_json::{json, Value};

const FIXTURE: &str = include_str!("fixtures/workflow_dsl_echo.yml");

fn snapshot() -> Vec<u8> {
    let source: Value = serde_yaml_ng::from_str(FIXTURE).expect("fixture");
    canonical_workflow_authoring_snapshot(source.to_string().as_bytes()).expect("snapshot")
}

fn apply(base: &[u8], operation: Value) -> Vec<u8> {
    apply_workflow_authoring_operation(base, operation.to_string().as_bytes()).expect("operation")
}

fn document(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("document JSON")
}

#[test]
fn canonical_snapshot_is_stable_and_preserves_extensions() {
    let first = snapshot();
    let second = canonical_workflow_authoring_snapshot(
        serde_json::to_string(&document(&first))
            .expect("JSON")
            .as_bytes(),
    )
    .expect("second snapshot");
    assert_eq!(first, second);
    let value = document(&first);
    assert_eq!(value["x-a3s-document-extension"]["retained"], true);
    assert_eq!(value["workflow"]["graph"]["viewport"]["zoom"], 1);
}

#[test]
fn applies_node_edge_and_app_operations_without_losing_unknown_fields() {
    let base = snapshot();
    let added = apply(
        &base,
        json!({
            "kind": "add-node",
            "id": "custom",
            "type": "host.custom",
            "configuration": {"answer": 42},
        }),
    );
    let added = document(&added);
    assert_eq!(added["workflow"]["graph"]["nodes"][3]["id"], "custom");
    assert_eq!(added["workflow"]["graph"]["nodes"][3]["data"]["answer"], 42);

    let added = apply(
        &serde_json::to_vec(&added).expect("added JSON"),
        json!({
            "kind": "add-edge",
            "id": "end-source-custom-target",
            "source": "end",
            "target": "custom",
            "sourceHandle": "source",
            "targetHandle": "target",
        }),
    );
    let added = apply(
        &added,
        json!({
            "kind": "set-edge",
            "id": "end-source-custom-target",
            "source": "end",
            "target": "custom",
            "sourceHandle": null,
            "targetHandle": null,
        }),
    );
    let added = apply(
        &added,
        json!({
            "kind": "set-node",
            "id": "custom",
            "configuration": {"answer": 7, "extension": {"kept": true}},
        }),
    );
    let added = apply(&added, json!({"kind": "set-app-name", "name": "renamed"}));
    let value = document(&added);
    assert_eq!(value["app"]["name"], "renamed");
    assert_eq!(value["workflow"]["graph"]["nodes"][3]["data"]["answer"], 7);
    assert_eq!(
        value["workflow"]["graph"]["nodes"][3]["data"]["extension"]["kept"],
        true
    );
    let edge = value["workflow"]["graph"]["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .find(|edge| edge["id"] == "end-source-custom-target")
        .expect("new edge");
    assert!(edge.get("sourceHandle").is_none());
    assert!(edge.get("targetHandle").is_none());
    assert_eq!(
        value["workflow"]["graph"]["nodes"][1]["data"]["x-a3s-fixture-extension"]["retained"],
        true
    );
}

#[test]
fn removes_descendants_and_incident_edges_and_rejects_parent_cycles() {
    let base = snapshot();
    let error = apply_workflow_authoring_operation(
        &base,
        json!({"kind": "move-node", "id": "start", "parentId": "start"})
            .to_string()
            .as_bytes(),
    )
    .expect_err("self-parent must fail");
    assert!(matches!(
        error,
        WorkflowDslError::InvalidAuthoringOperation { .. }
    ));

    let removed = apply(&base, json!({"kind": "remove-node", "id": "llm"}));
    let value = document(&removed);
    assert_eq!(
        value["workflow"]["graph"]["nodes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(value["workflow"]["graph"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .all(|edge| edge["source"] != "llm" && edge["target"] != "llm"));
}

#[test]
fn move_node_null_parent_clears_scope() {
    let base = snapshot();
    let container = apply(
        &base,
        json!({
            "kind": "add-node",
            "id": "each",
            "type": "iteration",
            "configuration": {"start_node_id": "each-start"},
        }),
    );
    let container = apply(
        &container,
        json!({
            "kind": "add-node",
            "id": "each-start",
            "type": "iteration-start",
            "parentId": "each",
        }),
    );
    let scoped = apply(
        &container,
        json!({
            "kind": "add-node",
            "id": "scoped",
            "type": "flow.step",
            "parentId": "each",
        }),
    );
    let moved = apply(
        &scoped,
        json!({
            "kind": "move-node",
            "id": "scoped",
            "parentId": null,
        }),
    );
    let value = document(&moved);
    let node = value["workflow"]["graph"]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == "scoped")
        .expect("scoped node");
    assert!(node.get("parentId").is_none());
}

#[test]
fn strict_bounds_and_executable_admission_are_explicit() {
    let base = snapshot();
    let unknown = apply_workflow_authoring_operation(
        &base,
        br#"{"kind":"set-app-name","name":"ok","extra":true}"#,
    )
    .expect_err("unknown field");
    assert!(matches!(
        unknown,
        WorkflowDslError::InvalidAuthoringOperation { .. }
    ));

    let oversized = vec![b' '; WORKFLOW_AUTHORING_OPERATION_MAX_BYTES + 1];
    let oversized =
        apply_workflow_authoring_operation(&base, &oversized).expect_err("oversized operation");
    assert!(matches!(
        oversized,
        WorkflowDslError::InvalidAuthoringOperation { .. }
    ));

    let mut draft = document(&base);
    draft["workflow"]["graph"]["nodes"] = json!([]);
    draft["workflow"]["graph"]["edges"] = json!([]);
    let draft = canonical_workflow_authoring_snapshot(
        serde_json::to_vec(&draft).expect("draft JSON").as_slice(),
    )
    .expect("draft snapshot");
    assert!(validate_executable_workflow_authoring_snapshot(&draft).is_err());
}

#[test]
fn strict_json_rejects_excessive_nesting_before_canonicalization() {
    let mut nested = serde_json::json!(null);
    for _ in 0..=256 {
        nested = serde_json::json!([nested]);
    }
    let mut value = document(&snapshot());
    value["x-deep-extension"] = nested;
    let error = canonical_workflow_authoring_snapshot(
        serde_json::to_vec(&value).expect("deep JSON").as_slice(),
    )
    .expect_err("excessive nesting must fail closed");
    assert!(matches!(error, WorkflowDslError::InvalidJson { .. }));
}

#[test]
fn malformed_utf8_and_failed_operations_never_mutate_the_base() {
    let base = snapshot();
    assert!(canonical_workflow_authoring_snapshot(&[0xff]).is_err());
    let error =
        apply_workflow_authoring_operation(&base, br#"{"kind":"remove-edge","id":"missing"}"#)
            .expect_err("missing edge");
    assert!(matches!(
        error,
        WorkflowDslError::InvalidAuthoringOperation { .. }
    ));
    assert_eq!(snapshot(), base);
}

#[test]
fn duplicate_json_keys_are_rejected_at_every_authoring_boundary() {
    let base = snapshot();
    let snapshot_error = canonical_workflow_authoring_snapshot(
        br#"{"version":"0.7.0","kind":"app","app":{"name":"Draft","mode":"workflow","name":"shadow"},"workflow":{"graph":{"nodes":[],"edges":[]}}}"#,
    )
    .expect_err("duplicate snapshot key");
    assert!(matches!(
        snapshot_error,
        WorkflowDslError::InvalidJson { .. }
    ));

    let operation_error = apply_workflow_authoring_operation(
        &base,
        br#"{"kind":"set-app-name","name":"one","n\u0061me":"two"}"#,
    )
    .expect_err("duplicate operation key");
    assert!(matches!(
        operation_error,
        WorkflowDslError::InvalidAuthoringOperation { .. }
    ));
}

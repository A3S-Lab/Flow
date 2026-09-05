use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventStore, FlowRuntime, InMemoryEventStore,
    RuntimeBuildCompatibility, RuntimeBuildId, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowPatchId, WorkflowRunStatus, WorkflowSpec, MAX_WORKFLOW_PATCH_MARKERS,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::sync::Arc;

const PATCH_ID: &str = "checkout.calculation-v2";
const ROLLOUT_GATE: &str = "rollout-gate";

fn resume_at() -> DateTime<Utc> {
    "2026-08-20T00:00:00Z".parse().unwrap()
}

fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("patch.workflow", "2", "tests::patch_markers", "main")
}

fn patched_workflow_spec() -> WorkflowSpec {
    workflow_spec().with_patch_marker(WorkflowPatchId::new(PATCH_ID).unwrap())
}

fn build_id(value: &str) -> RuntimeBuildId {
    RuntimeBuildId::new(value).unwrap()
}

struct PatchRuntime;

#[async_trait]
impl FlowRuntime for PatchRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();

        if !context.step_completed("prepare") {
            return Ok(context.schedule_step("prepare", "prepare", json!({})));
        }
        if !context.wait_completed(ROLLOUT_GATE) {
            return Ok(context.wait_until(ROLLOUT_GATE, resume_at()));
        }

        let (step_id, branch) = if context.has_patch_marker(PATCH_ID) {
            ("calculate-v2", "patched")
        } else {
            ("calculate-v1", "legacy")
        };
        if !context.step_completed(step_id) {
            return Ok(context.schedule_step(step_id, branch, json!({})));
        }

        Ok(context.complete(json!({ "branch": branch })))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<Value> {
        Ok(json!({ "step": invocation.step_name }))
    }
}

#[test]
fn workflow_patch_ids_are_bounded_typed_and_json_compatible() {
    let marker = WorkflowPatchId::new(PATCH_ID).unwrap();
    assert_eq!(marker.as_str(), PATCH_ID);
    assert_eq!(marker.to_string(), PATCH_ID);

    let legacy: WorkflowSpec = serde_json::from_value(json!({
        "name": "legacy.workflow",
        "version": "1",
        "runtime": {
            "kind": "rust_embedded",
            "entrypoint": "tests::legacy",
            "export_name": "main"
        }
    }))
    .unwrap();
    legacy.validate().unwrap();
    assert!(!legacy.has_patch_marker(PATCH_ID));
    assert!(
        serde_json::to_value(&legacy)
            .unwrap()
            .get("patch_markers")
            .is_none(),
        "empty markers must preserve the legacy wire shape"
    );

    let patched = patched_workflow_spec();
    patched.validate().unwrap();
    assert!(patched.has_patch_marker(PATCH_ID));
    assert_eq!(
        serde_json::to_value(&patched).unwrap()["patch_markers"],
        json!([PATCH_ID])
    );
    let sorted = workflow_spec()
        .with_patch_marker(WorkflowPatchId::new("checkout.tax-v2").unwrap())
        .with_patch_marker(WorkflowPatchId::new("checkout.address-v2").unwrap());
    assert_eq!(
        serde_json::to_value(&sorted).unwrap()["patch_markers"],
        json!(["checkout.address-v2", "checkout.tax-v2"]),
        "wire order must be canonical"
    );

    for invalid in [
        "",
        " checkout.v2",
        "checkout v2",
        "checkout/V2",
        "Checkout.v2",
        ".checkout.v2",
        "checkout.v2.",
    ] {
        assert!(
            WorkflowPatchId::new(invalid).is_err(),
            "accepted invalid patch marker {invalid:?}"
        );
    }

    let too_long = "a".repeat(129);
    assert!(WorkflowPatchId::new(too_long).is_err());

    let mut too_many = workflow_spec();
    for index in 0..=MAX_WORKFLOW_PATCH_MARKERS {
        too_many
            .patch_markers
            .insert(WorkflowPatchId::new(format!("patch.{index}")).unwrap());
    }
    assert!(matches!(
        too_many.validate(),
        Err(FlowError::InvalidWorkflow(message))
            if message.contains("patch marker count")
    ));

    let mut oversized_json = serde_json::to_value(workflow_spec()).unwrap();
    oversized_json["patch_markers"] = Value::Array(
        (0..=MAX_WORKFLOW_PATCH_MARKERS)
            .map(|index| json!(format!("patch.{index}")))
            .collect(),
    );
    assert!(serde_json::from_value::<WorkflowSpec>(oversized_json).is_err());

    let mut duplicate_json = serde_json::to_value(workflow_spec()).unwrap();
    duplicate_json["patch_markers"] = json!([PATCH_ID, PATCH_ID]);
    assert!(serde_json::from_value::<WorkflowSpec>(duplicate_json).is_err());
}

#[tokio::test]
async fn event_store_rejects_a_directly_written_oversized_marker_set() {
    let store = Arc::new(InMemoryEventStore::new());
    let mut invalid_spec = workflow_spec();
    for index in 0..=MAX_WORKFLOW_PATCH_MARKERS {
        invalid_spec
            .patch_markers
            .insert(WorkflowPatchId::new(format!("patch.{index}")).unwrap());
    }
    let error = store
        .append(
            "invalid-patch-history",
            FlowEvent::RunCreated {
                spec: invalid_spec,
                input: json!({}),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::InvalidWorkflow(message) if message.contains("patch marker count")
    ));
}

#[tokio::test]
async fn compatible_runtime_replays_legacy_and_patched_branches_from_pinned_markers() {
    let store = Arc::new(InMemoryEventStore::new());
    let build_v1 = build_id("patch-worker-v1");
    let build_v2 = build_id("patch-worker-v2");
    let legacy_engine = FlowEngine::builder(Arc::new(PatchRuntime))
        .with_store(store.clone())
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(build_v1.clone()))
        .build();

    legacy_engine
        .start_with_id(
            "legacy-run",
            workflow_spec().with_runtime_build(build_v1.clone()),
            json!({}),
        )
        .await
        .unwrap();

    let replacement = FlowEngine::builder(Arc::new(PatchRuntime))
        .with_store(store)
        .with_runtime_build_compatibility(
            RuntimeBuildCompatibility::new(build_v2.clone()).with_compatible_build(build_v1),
        )
        .build();
    replacement
        .start_with_id(
            "patched-run",
            patched_workflow_spec().with_runtime_build(build_v2),
            json!({}),
        )
        .await
        .unwrap();

    assert_eq!(
        replacement.snapshot("legacy-run").await.unwrap().status,
        WorkflowRunStatus::Suspended
    );
    assert_eq!(
        replacement.snapshot("patched-run").await.unwrap().status,
        WorkflowRunStatus::Suspended
    );

    replacement
        .resume_wait("legacy-run", ROLLOUT_GATE)
        .await
        .unwrap();
    replacement
        .resume_wait("patched-run", ROLLOUT_GATE)
        .await
        .unwrap();

    let legacy = replacement.snapshot("legacy-run").await.unwrap();
    assert_eq!(legacy.status, WorkflowRunStatus::Completed);
    assert_eq!(legacy.output, Some(json!({ "branch": "legacy" })));
    assert!(legacy.steps.contains_key("calculate-v1"));
    assert!(!legacy.steps.contains_key("calculate-v2"));

    let patched = replacement.snapshot("patched-run").await.unwrap();
    assert_eq!(patched.status, WorkflowRunStatus::Completed);
    assert_eq!(patched.output, Some(json!({ "branch": "patched" })));
    assert!(patched.spec.has_patch_marker(PATCH_ID));
    assert!(patched.steps.contains_key("calculate-v2"));
    assert!(!patched.steps.contains_key("calculate-v1"));
}

#[tokio::test]
async fn idempotent_start_rejects_patch_marker_drift_without_appending() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(PatchRuntime));

    engine
        .start_with_id("patch-drift", workflow_spec(), json!({}))
        .await
        .unwrap();
    let history_before = store.list("patch-drift").await.unwrap();

    let error = engine
        .start_with_id("patch-drift", patched_workflow_spec(), json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(
            &error,
            FlowError::RunConflict { run_id, reason }
                if run_id == "patch-drift" && reason == "workflow spec differs"
        ),
        "expected a workflow spec conflict, got {error:?}"
    );
    assert_eq!(
        store.list("patch-drift").await.unwrap(),
        history_before,
        "a patch marker cannot be added to an existing run"
    );
}

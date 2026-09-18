use a3s_flow::{
    FlowEngine, FlowRuntime, FlowVisibilityProjection, JsonValue, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowProgress, WorkflowSpec, FLOW_VISIBILITY_PROJECTION_SCHEMA_VERSION,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct VisRuntime;

#[async_trait]
impl FlowRuntime for VisRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.progress("half").is_none() {
            return Ok(ctx.record_progress(
                WorkflowProgress::new("half", 1)
                    .with_total(2)
                    .with_message("halfway")
                    .with_details(json!({"do_not_index": true})),
            ));
        }
        if !ctx.step_completed("work") {
            return Ok(ctx.schedule_step("work", "work", json!({})));
        }
        Ok(ctx.complete(json!({ "ok": true, "blob": "x".repeat(64) })))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
        Ok(json!({ "step": invocation.step_name }))
    }
}

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("visibility.demo", "1", "tests::visibility", "main")
}

#[tokio::test]
async fn visibility_projection_is_tip_anchored_and_rebuildable_from_history() {
    let engine = FlowEngine::in_memory(Arc::new(VisRuntime));
    engine
        .start_with_id("vis-run", spec(), json!({ "input": "large-payload" }))
        .await
        .unwrap();

    let live = engine.visibility_projection("vis-run").await.unwrap();
    live.validate().unwrap();
    assert_eq!(
        live.schema_version,
        FLOW_VISIBILITY_PROJECTION_SCHEMA_VERSION
    );
    assert_eq!(live.run_id, "vis-run");
    assert_eq!(live.workflow_name, "visibility.demo");
    assert_eq!(live.status, a3s_flow::WorkflowRunStatus::Completed);
    assert_eq!(
        live.latest_progress
            .as_ref()
            .map(|p| p.progress_id.as_str()),
        Some("half")
    );
    assert!(live
        .latest_progress
        .as_ref()
        .is_some_and(|p| { serde_json::to_value(p).unwrap().get("details").is_none() }));

    let history = engine.history("vis-run").await.unwrap();
    let rebuilt = FlowVisibilityProjection::from_history("vis-run", &history).unwrap();
    assert_eq!(rebuilt, live);

    let encoded = serde_json::to_value(&live).unwrap();
    assert!(encoded.get("input").is_none());
    assert!(encoded.get("output").is_none());
    assert!(encoded.get("steps").is_none());
    assert!(encoded.get("activities").is_none());
}

#[tokio::test]
async fn visibility_projection_matches_checkpoint_snapshot_digest() {
    let engine = FlowEngine::in_memory(Arc::new(VisRuntime));
    engine
        .start_with_id("vis-checkpoint", spec(), json!({}))
        .await
        .unwrap();

    let checkpoint = engine.checkpoint("vis-checkpoint").await.unwrap();
    let visibility = engine
        .visibility_projection("vis-checkpoint")
        .await
        .unwrap();

    assert_eq!(visibility.last_sequence, checkpoint.last_sequence);
    assert_eq!(visibility.last_event_id, checkpoint.last_event_id);
    assert_eq!(visibility.snapshot_sha256, checkpoint.snapshot_sha256);
}

#[tokio::test]
async fn visibility_projection_rebuilds_from_tip_validated_checkpoint_without_full_history() {
    use a3s_flow::FlowEventStore;

    let store = Arc::new(a3s_flow::InMemoryEventStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(VisRuntime));
    engine
        .start_with_id("vis-checkpoint-rebuild", spec(), json!({ "n": 1 }))
        .await
        .unwrap();

    let live = engine
        .visibility_projection("vis-checkpoint-rebuild")
        .await
        .unwrap();
    let checkpoint = engine.checkpoint("vis-checkpoint-rebuild").await.unwrap();
    let loaded = store
        .load_checkpoint("vis-checkpoint-rebuild")
        .await
        .unwrap()
        .expect("checkpoint must be durable for host index rebuild");
    assert_eq!(loaded.last_sequence, checkpoint.last_sequence);
    assert_eq!(loaded.last_event_id, checkpoint.last_event_id);

    // Hosts that retained a tip-validated checkpoint can rebuild the visibility
    // contract from the cached snapshot without paging the full event log.
    let rebuilt =
        FlowVisibilityProjection::from_snapshot(loaded.snapshot.clone(), loaded.last_event_id)
            .unwrap();
    rebuilt.validate().unwrap();
    assert_eq!(rebuilt, live);
    assert_eq!(rebuilt.snapshot_sha256, loaded.snapshot_sha256);
}

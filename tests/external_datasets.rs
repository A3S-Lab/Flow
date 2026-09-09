use a3s_flow::{
    ExternalDatasetRef, FlowEngine, FlowError, FlowRuntime, JsonValue, RuntimeCommand,
    StepInvocation, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn dataset_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("dataset.demo", "1", "tests::external_datasets", "main")
}

#[tokio::test]
async fn external_dataset_attach_is_durable_and_visible_to_replay() {
    struct DatasetRuntime;
    #[async_trait]
    impl FlowRuntime for DatasetRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> a3s_flow::Result<RuntimeCommand> {
            let ctx = invocation.context();
            if ctx.external_dataset("orders").is_none() {
                return Ok(ctx.attach_external_dataset(ExternalDatasetRef::new(
                    "orders",
                    "sha256:abc123",
                    1_000,
                )));
            }
            let dataset = ctx.external_dataset("orders").unwrap();
            Ok(ctx.complete(json!({
                "datasetId": dataset.dataset_id,
                "digest": dataset.content_digest,
                "itemCount": dataset.item_count,
            })))
        }

        async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
            unreachable!()
        }
    }

    let engine = FlowEngine::in_memory(Arc::new(DatasetRuntime));
    let run_id = engine
        .start_with_id("dataset-ok", dataset_spec(), json!({}))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(
        snapshot.output,
        Some(json!({
            "datasetId": "orders",
            "digest": "sha256:abc123",
            "itemCount": 1000,
        }))
    );
    assert_eq!(
        snapshot.external_dataset("orders"),
        Some(&ExternalDatasetRef::new("orders", "sha256:abc123", 1_000))
    );
}

#[tokio::test]
async fn external_dataset_attach_is_idempotent_and_rejects_digest_drift() {
    struct DriftRuntime {
        phase: AtomicUsize,
    }
    #[async_trait]
    impl FlowRuntime for DriftRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> a3s_flow::Result<RuntimeCommand> {
            let ctx = invocation.context();
            let phase = self.phase.fetch_add(1, Ordering::SeqCst);
            let digest = if phase == 0 {
                "sha256:abc123"
            } else {
                "sha256:DRIFT"
            };
            Ok(ctx.attach_external_dataset(ExternalDatasetRef::new("orders", digest, 10)))
        }

        async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
            unreachable!()
        }
    }

    let engine = FlowEngine::in_memory(Arc::new(DriftRuntime {
        phase: AtomicUsize::new(0),
    }));
    let err = engine
        .start_with_id("dataset-drift", dataset_spec(), json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, FlowError::NonDeterministic { ref reason, .. } if reason.contains("differs")),
        "{err:?}"
    );
}

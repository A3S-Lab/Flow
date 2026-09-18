use a3s_flow::{
    FlowBlobEncryption, FlowBlobRef, FlowEngine, FlowError, FlowRuntime, JsonValue, RuntimeCommand,
    StepInvocation, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec, MAX_FLOW_EVENT_BYTES,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn blob_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("blob.demo", "1", "tests::blob_refs", "main")
}

#[tokio::test]
async fn blob_ref_attach_is_durable_and_visible_to_replay() {
    struct BlobRuntime;
    #[async_trait]
    impl FlowRuntime for BlobRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> a3s_flow::Result<RuntimeCommand> {
            let ctx = invocation.context();
            if ctx.blob_ref("result").is_none() {
                return Ok(ctx.attach_blob_ref(
                    FlowBlobRef::new("result", "sha256:abc123")
                        .with_codec("gzip")
                        .with_byte_length(1_024)
                        .with_encryption(FlowBlobEncryption::new("aes-256-gcm", "key-1")),
                ));
            }
            let blob = ctx.blob_ref("result").unwrap();
            Ok(ctx.complete(json!({
                "blobId": blob.blob_id,
                "digest": blob.content_digest,
                "codec": blob.codec,
            })))
        }

        async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
            unreachable!()
        }
    }

    let engine = FlowEngine::in_memory(Arc::new(BlobRuntime));
    let run_id = engine
        .start_with_id("blob-ok", blob_spec(), json!({}))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(
        snapshot.blob_ref("result"),
        Some(
            &FlowBlobRef::new("result", "sha256:abc123")
                .with_codec("gzip")
                .with_byte_length(1_024)
                .with_encryption(FlowBlobEncryption::new("aes-256-gcm", "key-1"))
        )
    );
}

#[tokio::test]
async fn blob_ref_attach_is_idempotent_and_rejects_digest_drift() {
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
            Ok(ctx.attach_blob_ref(FlowBlobRef::new("result", digest)))
        }

        async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
            unreachable!()
        }
    }

    let engine = FlowEngine::in_memory(Arc::new(DriftRuntime {
        phase: AtomicUsize::new(0),
    }));
    let err = engine
        .start_with_id("blob-drift", blob_spec(), json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, FlowError::NonDeterministic { ref reason, .. } if reason.contains("differs")),
        "{err:?}"
    );
}

#[tokio::test]
async fn step_output_blob_ref_marker_persists_under_event_budget() {
    struct StepBlobRuntime;
    #[async_trait]
    impl FlowRuntime for StepBlobRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> a3s_flow::Result<RuntimeCommand> {
            let ctx = invocation.context();
            if ctx.step_output("load").is_none() {
                return Ok(ctx.schedule_step("load", "load-blob", json!({})));
            }
            let output = ctx.step_output("load").unwrap().clone();
            Ok(ctx.complete(output))
        }

        async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
            Ok(FlowBlobRef::new("payload", "sha256:step-out")
                .with_codec("identity")
                .with_byte_length(8_000_000)
                .to_output_value())
        }
    }

    let engine = FlowEngine::in_memory(Arc::new(StepBlobRuntime));
    let run_id = engine
        .start_with_id("blob-step", blob_spec(), json!({}))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    let output = snapshot.output.as_ref().unwrap();
    let parsed = FlowBlobRef::try_from_output_value(output).unwrap();
    assert_eq!(
        parsed,
        Some(
            FlowBlobRef::new("payload", "sha256:step-out")
                .with_codec("identity")
                .with_byte_length(8_000_000)
        )
    );
}

#[tokio::test]
async fn oversized_inline_output_still_fails_closed() {
    struct HugeRuntime;
    #[async_trait]
    impl FlowRuntime for HugeRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> a3s_flow::Result<RuntimeCommand> {
            Ok(invocation
                .context()
                .complete(json!("x".repeat(MAX_FLOW_EVENT_BYTES))))
        }

        async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
            unreachable!()
        }
    }

    let engine = FlowEngine::in_memory(Arc::new(HugeRuntime));
    let err = engine
        .start_with_id("blob-too-large", blob_spec(), json!({}))
        .await
        .unwrap_err();
    assert!(matches!(err, FlowError::PayloadTooLarge { .. }), "{err:?}");
}

use a3s_flow::{
    ChildWorkflowCommand, FlowEngine, FlowError, FlowRuntime, ItemAggregateContribution, JsonValue,
    RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
    WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn parent_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("aggregate.parent", "1", "tests::item_aggregates", "parent")
}

fn child_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("aggregate.child", "1", "tests::item_aggregates", "child")
}

#[tokio::test]
async fn item_aggregate_records_map_outcomes_in_durable_order() {
    struct AggregateRuntime;
    #[async_trait]
    impl FlowRuntime for AggregateRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> a3s_flow::Result<RuntimeCommand> {
            let ctx = invocation.context();
            if invocation.spec.name == "aggregate.parent" {
                let children = (0..3)
                    .map(|ordinal| {
                        ChildWorkflowCommand::new(
                            format!("item-{ordinal}"),
                            child_spec(),
                            json!({ "ordinal": ordinal }),
                        )
                    })
                    .collect::<Vec<_>>();

                if !ctx.child_workflow_map_completed("items") {
                    return Ok(ctx.map_child_workflows("items", children.clone(), 2));
                }

                for child in &children {
                    if !ctx.item_aggregate_has("results", &child.child_id) {
                        let outcome =
                            ctx.child_workflow_outcome(&child.child_id).ok_or_else(|| {
                                FlowError::Runtime(format!("missing outcome {}", child.child_id))
                            })?;
                        let value = match outcome {
                            WorkflowTerminalOutcome::Completed { output } => output.clone(),
                            other => {
                                return Err(FlowError::Runtime(format!("child failed: {other:?}")));
                            }
                        };
                        return Ok(ctx.record_item_aggregate(ItemAggregateContribution::new(
                            "results",
                            child.child_id.clone(),
                            value,
                        )));
                    }
                }

                let values = ctx.item_aggregate_values("results");
                return Ok(ctx.complete(json!({ "results": values })));
            }

            Ok(ctx.complete(json!({ "ordinal": ctx.input()["ordinal"] })))
        }

        async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
            unreachable!()
        }
    }

    let engine = FlowEngine::in_memory(Arc::new(AggregateRuntime));
    let run_id = engine
        .start_with_id("agg-order", parent_spec(), json!({}))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(
        snapshot.output,
        Some(json!({
            "results": [
                { "ordinal": 0 },
                { "ordinal": 1 },
                { "ordinal": 2 },
            ]
        }))
    );
    let aggregate = snapshot.item_aggregate("results").unwrap();
    assert_eq!(aggregate.len(), 3);
    assert_eq!(aggregate.get("item-1"), Some(&json!({ "ordinal": 1 })));
}

#[tokio::test]
async fn item_aggregate_is_idempotent_and_rejects_value_drift() {
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
            let value = if phase == 0 {
                json!({ "ok": true })
            } else {
                json!({ "ok": false })
            };
            Ok(ctx
                .record_item_aggregate(ItemAggregateContribution::new("results", "item-0", value)))
        }

        async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
            unreachable!()
        }
    }

    let engine = FlowEngine::in_memory(Arc::new(DriftRuntime {
        phase: AtomicUsize::new(0),
    }));
    let err = engine
        .start_with_id("agg-drift", parent_spec(), json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(err, FlowError::NonDeterministic { ref reason, .. } if reason.contains("differs")),
        "{err:?}"
    );
}

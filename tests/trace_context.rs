use a3s_flow::{
    with_trace_context, FlowEngine, FlowError, FlowRuntime, FlowTraceContext, JsonValue,
    RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::{Arc, Mutex};

struct TraceRuntime {
    seen: Arc<Mutex<Option<FlowTraceContext>>>,
}

#[async_trait]
impl FlowRuntime for TraceRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        *self.seen.lock().unwrap() = invocation.trace_context.clone();
        let ctx = invocation.context();
        if !ctx.step_completed("work") {
            return Ok(ctx.schedule_step("work", "work", json!({})));
        }
        Ok(ctx.complete(json!({ "ok": true })))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<JsonValue> {
        assert!(invocation.trace_context.is_some());
        Ok(json!({ "step": invocation.step_name }))
    }
}

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("trace.demo", "1", "tests::trace_context", "main")
}

#[tokio::test]
async fn ambient_trace_context_propagates_to_workflow_and_step_invocations() {
    let seen = Arc::new(Mutex::new(None));
    let engine = FlowEngine::in_memory(Arc::new(TraceRuntime { seen: seen.clone() }));
    let trace = FlowTraceContext::new(
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        Some("rojo=00f067aa0ba902b7".into()),
    )
    .unwrap();

    with_trace_context(trace.clone(), async {
        engine
            .start_with_id("trace-run", spec(), json!({}))
            .await
            .unwrap();
    })
    .await;

    assert_eq!(seen.lock().unwrap().as_ref(), Some(&trace));
}

#[test]
fn invalid_traceparent_is_rejected() {
    assert!(matches!(
        FlowTraceContext::new("not-a-traceparent", None),
        Err(FlowError::InvalidTransition(_))
    ));
}

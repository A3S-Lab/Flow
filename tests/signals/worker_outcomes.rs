use super::*;

const SIGNAL_ID: &str = "approval-delivery-1";

fn approval_task(run_id: &str) -> FlowTask {
    FlowTask::SendSignal {
        run_id: run_id.to_string(),
        signal: WorkflowSignal::new(SIGNAL_ID, APPROVAL_SIGNAL, json!({ "approved": true })),
    }
}

fn delivered_signals(outcomes: &[a3s_flow::FlowTaskOutcome]) -> Vec<(String, String)> {
    outcomes
        .iter()
        .filter_map(|outcome| outcome.delivered_signal.clone())
        .collect()
}

struct CoordinatedSignalStore {
    inner: a3s_flow::InMemoryEventStore,
    delivery_barrier: tokio::sync::Barrier,
}

impl CoordinatedSignalStore {
    fn new() -> Self {
        Self {
            inner: a3s_flow::InMemoryEventStore::new(),
            delivery_barrier: tokio::sync::Barrier::new(2),
        }
    }
}

#[async_trait]
impl FlowEventStore for CoordinatedSignalStore {
    async fn append(
        &self,
        run_id: &str,
        event: FlowEvent,
    ) -> a3s_flow::Result<a3s_flow::FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<a3s_flow::FlowEventEnvelope> {
        if matches!(&event, FlowEvent::SignalReceived { .. }) {
            self.delivery_barrier.wait().await;
        }
        self.inner
            .append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<a3s_flow::FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

#[tokio::test]
async fn queued_signal_redelivery_reports_only_the_committed_delivery() {
    let engine = FlowEngine::in_memory(Arc::new(ApprovalRuntime));
    let run_id = engine
        .start_with_id("signal-queued-redelivery", spec(), json!({}))
        .await
        .unwrap();
    let worker = FlowWorker::in_memory(engine.clone());
    let task = approval_task(&run_id);
    assert_eq!(task.target_run_id(), Some(run_id.as_str()));
    let encoded = serde_json::to_string(&task).unwrap();
    assert_eq!(
        encoded,
        format!(
            r#"{{"type":"send_signal","run_id":"{run_id}","signal":{{"signal_id":"approval-delivery-1","name":"order.approved","payload":{{"approved":true}}}}}}"#
        )
    );
    assert_eq!(serde_json::from_str::<FlowTask>(&encoded).unwrap(), task);
    worker.enqueue(task.clone()).await.unwrap();
    worker.enqueue(task).await.unwrap();

    let outcomes = worker.run_until_idle().await.unwrap();

    assert_eq!(outcomes.len(), 2);
    assert!(outcomes
        .iter()
        .all(|outcome| outcome.run_ids == vec![run_id.clone()]));
    assert_eq!(
        delivered_signals(&outcomes),
        vec![(run_id.clone(), SIGNAL_ID.to_string())]
    );
    assert_eq!(
        engine
            .history(&run_id)
            .await
            .unwrap()
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::SignalReceived { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn concurrent_signal_redelivery_reports_only_the_committed_delivery() {
    let store = Arc::new(CoordinatedSignalStore::new());
    let engine = FlowEngine::new(store, Arc::new(ApprovalRuntime));
    let run_id = engine
        .start_with_id("signal-concurrent-worker-redelivery", spec(), json!({}))
        .await
        .unwrap();
    let worker = FlowWorker::in_memory(engine.clone());
    let task = approval_task(&run_id);

    let (first, second) = tokio::join!(worker.handle(task.clone()), worker.handle(task));
    let outcomes = [first.unwrap(), second.unwrap()];

    assert!(outcomes
        .iter()
        .all(|outcome| outcome.run_ids == vec![run_id.clone()]));
    assert_eq!(
        delivered_signals(&outcomes),
        vec![(run_id.clone(), SIGNAL_ID.to_string())]
    );
    assert_eq!(
        engine
            .history(&run_id)
            .await
            .unwrap()
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::SignalReceived { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn continued_signal_redelivery_reports_only_the_leaf_commit() {
    let engine = FlowEngine::in_memory(Arc::new(ContinueThenSignalRuntime));
    let root_run_id = engine
        .start_with_id(
            "signal-worker-continuation-root",
            spec(),
            json!({ "generation": 0 }),
        )
        .await
        .unwrap();
    let leaf_run_id = engine.continuation_chain(&root_run_id).await.unwrap()[1]
        .run_id
        .clone();
    let worker = FlowWorker::in_memory(engine);
    let task = approval_task(&root_run_id);

    let first = worker.handle(task.clone()).await.unwrap();
    let repeated = worker.handle(task).await.unwrap();

    assert_eq!(first.run_ids, vec![leaf_run_id.clone()]);
    assert_eq!(
        first.delivered_signal,
        Some((leaf_run_id.clone(), SIGNAL_ID.to_string()))
    );
    assert_eq!(repeated.run_ids, vec![leaf_run_id]);
    assert_eq!(repeated.delivered_signal, None);
}

struct SignalThenContinueRuntime;

#[async_trait]
impl FlowRuntime for SignalThenContinueRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.input()["generation"] == 0 {
            if context.signal_payload("approval").is_none() {
                return Ok(context.wait_for_signal("approval", APPROVAL_SIGNAL));
            }
            return Ok(context.continue_as_new(json!({ "generation": 1 })));
        }
        Ok(context.complete(json!({ "continued": true })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("signal workflow does not execute steps")
    }
}

#[tokio::test]
async fn signal_outcome_identifies_the_stream_that_committed_before_continuation() {
    let engine = FlowEngine::in_memory(Arc::new(SignalThenContinueRuntime));
    let root_run_id = engine
        .start_with_id(
            "signal-worker-continues-after-delivery",
            spec(),
            json!({ "generation": 0 }),
        )
        .await
        .unwrap();
    let worker = FlowWorker::in_memory(engine.clone());

    let outcome = worker.handle(approval_task(&root_run_id)).await.unwrap();
    let chain = engine.continuation_chain(&root_run_id).await.unwrap();
    assert_eq!(chain.len(), 2);
    let active_run_id = chain[1].run_id.clone();

    assert_eq!(outcome.run_ids, vec![active_run_id]);
    assert_eq!(
        outcome.delivered_signal,
        Some((root_run_id, SIGNAL_ID.to_string()))
    );
}

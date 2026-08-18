use super::*;

#[tokio::test]
async fn drives_steps_until_complete() {
    let engine = FlowEngine::in_memory(Arc::new(SequentialRuntime));
    let run_id = engine
        .start(spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.steps.len(), 2);
    assert_eq!(snapshot.steps["load-user"].status, StepStatus::Completed);
    assert_eq!(snapshot.steps["send-email"].status, StepStatus::Completed);
    assert_eq!(snapshot.output.unwrap()["email"]["sent"], true);

    let keys: Vec<_> = engine
        .store()
        .list(&run_id)
        .await
        .unwrap()
        .into_iter()
        .map(|event| event.event.event_key())
        .collect();
    assert_eq!(
        keys,
        vec![
            "flow.run.created",
            "flow.run.started",
            "flow.step.created",
            "flow.step.started",
            "flow.step.completed",
            "flow.step.created",
            "flow.step.started",
            "flow.step.completed",
            "flow.run.completed",
        ]
    );
}

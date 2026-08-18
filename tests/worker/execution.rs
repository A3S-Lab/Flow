use super::*;

#[tokio::test]
async fn worker_resumes_due_waits_from_queue() {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let due_run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();
    let future_run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now + ChronoDuration::hours(1)).to_rfc3339() }),
        )
        .await
        .unwrap();

    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let worker = FlowWorker::new(engine.clone(), queue.clone());
    queue
        .enqueue(FlowTask::ResumeDueWaits { now })
        .await
        .unwrap();

    let outcome = worker.run_once().await.unwrap().unwrap();
    assert_eq!(
        outcome.resumed_waits,
        vec![(due_run_id.clone(), "sleep".to_string())]
    );
    assert_eq!(outcome.run_ids, vec![due_run_id.clone()]);
    assert!(queue.is_empty().await.unwrap());

    let due = engine.snapshot(&due_run_id).await.unwrap();
    assert_eq!(due.status, WorkflowRunStatus::Completed);
    assert_eq!(due.waits["sleep"].status, WaitStatus::Completed);

    let future = engine.snapshot(&future_run_id).await.unwrap();
    assert_eq!(future.status, WorkflowRunStatus::Suspended);
    assert_eq!(future.waits["sleep"].status, WaitStatus::Waiting);
}

#[tokio::test]
async fn worker_acknowledges_cancelled_wait_redelivery_without_reporting_resume() {
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let run_id = engine
        .start(
            spec(),
            json!({
                "resume_at": (Utc::now() + ChronoDuration::hours(1)).to_rfc3339()
            }),
        )
        .await
        .unwrap();
    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let worker = FlowWorker::new(engine.clone(), queue.clone());
    queue
        .enqueue(FlowTask::ResumeWait {
            run_id: run_id.clone(),
            wait_id: "sleep".to_string(),
        })
        .await
        .unwrap();
    engine
        .force_cancel(&run_id, Some("timer withdrawn".to_string()))
        .await
        .unwrap();

    let outcome = worker.run_once().await.unwrap().unwrap();

    assert_eq!(outcome.run_ids, vec![run_id.clone()]);
    assert!(outcome.resumed_waits.is_empty());
    assert!(queue.is_empty().await.unwrap());
    assert_eq!(
        engine.snapshot(&run_id).await.unwrap().status,
        WorkflowRunStatus::Cancelled
    );
}

#[tokio::test]
async fn worker_resumes_only_the_targeted_scheduled_run() {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let targeted_run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(2)).to_rfc3339() }),
        )
        .await
        .unwrap();
    let other_due_run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();

    let worker = FlowWorker::in_memory(engine.clone());
    worker
        .enqueue(FlowTask::ResumeScheduledRun {
            run_id: targeted_run_id.clone(),
            now,
        })
        .await
        .unwrap();

    let outcome = worker.run_once().await.unwrap().unwrap();
    assert_eq!(outcome.run_ids, vec![targeted_run_id.clone()]);
    assert_eq!(
        outcome.resumed_waits,
        vec![(targeted_run_id.clone(), "sleep".to_string())]
    );
    assert!(outcome.resumed_retries.is_empty());
    assert_eq!(
        engine.snapshot(&targeted_run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(
        engine.snapshot(&other_due_run_id).await.unwrap().status,
        WorkflowRunStatus::Suspended
    );
}

#[tokio::test]
async fn worker_resumes_due_retries_from_queue() {
    let now = Utc::now();
    let runtime = Arc::new(DelayedRetryRuntime::default());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({})).await.unwrap();
    let waiting = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(runtime.attempts.load(Ordering::SeqCst), 1);

    let worker = FlowWorker::in_memory(engine.clone());
    worker
        .enqueue(FlowTask::ResumeDueRetries {
            now: now + ChronoDuration::seconds(120),
        })
        .await
        .unwrap();

    let outcomes = worker.run_until_idle().await.unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].resumed_retries,
        vec![(run_id.clone(), "flaky".to_string())]
    );

    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.output.unwrap()["attempt"], 2);
}

#[tokio::test]
async fn worker_resumes_hook_by_token_from_queue() {
    let engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    let run_id = engine
        .start(spec(), json!({ "token": "approval-token" }))
        .await
        .unwrap();
    let waiting = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(waiting.hooks["approval"].status, HookStatus::Active);

    let worker = FlowWorker::in_memory(engine.clone());
    worker
        .enqueue(FlowTask::ResumeHookByToken {
            token: "approval-token".to_string(),
            payload: json!({ "approved": true }),
        })
        .await
        .unwrap();

    let outcomes = worker.run_until_idle().await.unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].resumed_hook,
        Some((run_id.clone(), "approval".to_string()))
    );
    assert_eq!(outcomes[0].run_ids, vec![run_id.clone()]);

    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.output.unwrap()["approved"], true);
}

#[tokio::test]
async fn worker_disposes_hook_by_token_from_queue() {
    let engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    let run_id = engine
        .start(spec(), json!({ "token": "approval-token" }))
        .await
        .unwrap();
    let waiting = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(waiting.hooks["approval"].status, HookStatus::Active);

    let worker = FlowWorker::in_memory(engine.clone());
    worker
        .enqueue(FlowTask::DisposeHookByToken {
            token: "approval-token".to_string(),
        })
        .await
        .unwrap();

    let outcomes = worker.run_until_idle().await.unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].disposed_hook,
        Some((run_id.clone(), "approval".to_string()))
    );
    assert_eq!(outcomes[0].run_ids, vec![run_id.clone()]);

    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.hooks["approval"].status, HookStatus::Disposed);
    assert_eq!(completed.output.unwrap()["status"], "disposed");
}

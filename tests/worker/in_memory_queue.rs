use super::*;

#[tokio::test]
async fn in_memory_task_queue_is_fifo() {
    let queue = InMemoryFlowTaskQueue::new();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "first".to_string(),
        })
        .await
        .unwrap();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "second".to_string(),
        })
        .await
        .unwrap();

    assert_eq!(queue.len().await.unwrap(), 2);
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::DriveRun {
            run_id: "first".to_string()
        })
    );
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::DriveRun {
            run_id: "second".to_string()
        })
    );
    assert_eq!(queue.dequeue().await.unwrap(), None);
}

#[tokio::test]
async fn in_memory_task_queue_rotates_heartbeat_fence_and_rejects_stale_ack() {
    let queue = InMemoryFlowTaskQueue::new();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "fenced".to_string(),
        })
        .await
        .unwrap();

    let lease = queue.lease().await.unwrap().unwrap();
    let renewed_lease_id = queue.heartbeat(&lease.lease_id).await.unwrap();
    assert_ne!(renewed_lease_id, lease.lease_id);
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    let err = queue.ack(&lease.lease_id).await.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(lease_id) if lease_id == lease.lease_id));
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    queue.ack(&renewed_lease_id).await.unwrap();
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    let err = queue.ack(&renewed_lease_id).await.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(lease_id) if lease_id == renewed_lease_id));
}

#[tokio::test]
async fn worker_bounded_drain_preserves_fairness_budget() {
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    for _ in 0..3 {
        queue
            .enqueue(FlowTask::ResumeDueWaits { now: Utc::now() })
            .await
            .unwrap();
    }
    let worker = FlowWorker::new(engine, queue.clone());
    let mut incompatible = worker.capabilities();
    incompatible.protocol = "a3s.flow.worker.v0".to_string();
    assert!(matches!(
        worker.ensure_compatible(&incompatible),
        Err(FlowError::UnsupportedWorkerProtocol { .. })
    ));
    assert_eq!(queue.len().await.unwrap(), 3);
    let outcomes = worker.run_until_idle_bounded(2).await.unwrap();
    assert_eq!(outcomes.len(), 2);
    assert_eq!(queue.len().await.unwrap(), 1);
    let error = worker.run_until_idle_bounded(0).await.unwrap_err();
    assert!(matches!(error, FlowError::InvalidWorkerConfiguration(_)));
    assert_eq!(queue.len().await.unwrap(), 1);
}

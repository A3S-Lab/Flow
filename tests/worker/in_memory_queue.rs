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

#[tokio::test]
async fn in_memory_task_queue_refuses_enqueue_at_pending_capacity() {
    let queue = InMemoryFlowTaskQueue::new().with_max_pending(1).unwrap();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "kept".to_string(),
        })
        .await
        .unwrap();
    let error = queue
        .enqueue(FlowTask::DriveRun {
            run_id: "rejected".to_string(),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::QueueBackpressure {
            pending: 1,
            capacity: 1
        }
    ));
    assert_eq!(queue.len().await.unwrap(), 1);
    assert_eq!(queue.max_pending_tasks(), Some(1));
    assert!(InMemoryFlowTaskQueue::new().with_max_pending(0).is_err());
}

#[tokio::test]
async fn in_memory_partition_fairness_rotates_across_opaque_keys() {
    let queue = InMemoryFlowTaskQueue::new().with_partition_fairness();
    assert!(queue.partition_fairness());

    for run_id in ["a-1", "a-2", "a-3"] {
        queue
            .enqueue_for_partition(
                "tenant-a",
                FlowTask::DriveRun {
                    run_id: run_id.to_string(),
                },
            )
            .await
            .unwrap();
    }
    queue
        .enqueue_for_partition(
            "tenant-b",
            FlowTask::DriveRun {
                run_id: "b-1".to_string(),
            },
        )
        .await
        .unwrap();

    let first = queue.lease().await.unwrap().unwrap();
    queue.ack(&first.lease_id).await.unwrap();
    let second = queue.lease().await.unwrap().unwrap();
    queue.ack(&second.lease_id).await.unwrap();

    assert_eq!(
        first.task,
        FlowTask::DriveRun {
            run_id: "a-1".to_string()
        }
    );
    assert_eq!(
        second.task,
        FlowTask::DriveRun {
            run_id: "b-1".to_string()
        }
    );
}

#[tokio::test]
async fn in_memory_partition_fairness_defaults_to_run_id_buckets() {
    let queue = InMemoryFlowTaskQueue::new().with_partition_fairness();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "hot".to_string(),
        })
        .await
        .unwrap();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "hot".to_string(),
        })
        .await
        .unwrap();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "cold".to_string(),
        })
        .await
        .unwrap();

    let first = queue.lease().await.unwrap().unwrap();
    queue.ack(&first.lease_id).await.unwrap();
    let second = queue.lease().await.unwrap().unwrap();
    queue.ack(&second.lease_id).await.unwrap();

    assert_eq!(
        first.task,
        FlowTask::DriveRun {
            run_id: "cold".to_string()
        }
    );
    assert_eq!(
        second.task,
        FlowTask::DriveRun {
            run_id: "hot".to_string()
        }
    );
}

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

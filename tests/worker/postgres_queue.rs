use super::*;

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_task_queue_leases_requeues_and_dead_letters_when_url_is_configured() {
    let Some(url) = postgres_url_from_env() else {
        eprintln!("skipping postgres queue integration test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let queue_name = format!("test-queue-{}", Uuid::new_v4());
    let queue = PostgresFlowTaskQueue::connect_with_queue(&url, &queue_name)
        .await
        .unwrap();
    let task = FlowTask::DriveRun {
        run_id: "postgres-poison-run".to_string(),
    };

    queue.enqueue(task.clone()).await.unwrap();
    assert_eq!(queue.queue_name(), queue_name);
    assert_eq!(queue.len().await.unwrap(), 1);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);

    let first_lease = queue.lease().await.unwrap().unwrap();
    assert_eq!(first_lease.task, task);
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    assert_eq!(
        queue
            .requeue_inflight_older_than(Utc::now() - ChronoDuration::seconds(1))
            .await
            .unwrap(),
        0
    );
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    assert_eq!(
        queue
            .requeue_inflight_older_than(Utc::now() + ChronoDuration::seconds(1))
            .await
            .unwrap(),
        1
    );
    assert_eq!(queue.len().await.unwrap(), 1);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);

    let second_lease = queue.lease().await.unwrap().unwrap();
    assert_eq!(second_lease.task, task);
    let second_lease_id = queue.heartbeat(&second_lease.lease_id).await.unwrap();
    assert_ne!(second_lease_id, second_lease.lease_id);

    let err = queue.ack(&first_lease.lease_id).await.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(lease_id) if lease_id == first_lease.lease_id));
    assert_eq!(queue.inflight_len().await.unwrap(), 1);
    assert_eq!(
        queue
            .dead_letter_inflight_older_than(
                Utc::now() + ChronoDuration::seconds(1),
                "lease expired after worker failure",
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.dead_letter_len().await.unwrap(), 1);

    let dead = queue.dead_lettered_tasks().await.unwrap();
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0].lease_id, second_lease_id);
    assert_eq!(dead[0].task, task);
    assert_eq!(dead[0].reason, "lease expired after worker failure");

    let err = queue.ack(&second_lease_id).await.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(lease_id) if lease_id == second_lease_id));
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_task_queue_competing_workers_lease_distinct_tasks_when_url_is_configured() {
    let Some(url) = postgres_url_from_env() else {
        eprintln!("skipping postgres competing-worker test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let queue_name = format!("test-competing-workers-{}", Uuid::new_v4());
    let first_queue = PostgresFlowTaskQueue::connect_with_queue(&url, &queue_name)
        .await
        .unwrap();
    let second_queue = PostgresFlowTaskQueue::connect_with_queue(&url, &queue_name)
        .await
        .unwrap();
    first_queue
        .enqueue(FlowTask::DriveRun {
            run_id: "first".to_string(),
        })
        .await
        .unwrap();
    first_queue
        .enqueue(FlowTask::DriveRun {
            run_id: "second".to_string(),
        })
        .await
        .unwrap();

    let (first, second) = tokio::join!(first_queue.lease(), second_queue.lease());
    let first = first.unwrap().unwrap();
    let second = second.unwrap().unwrap();
    assert_ne!(first.lease_id, second.lease_id);
    assert_ne!(first.task, second.task);
    assert_eq!(first_queue.inflight_len().await.unwrap(), 2);

    first_queue.ack(&first.lease_id).await.unwrap();
    second_queue.ack(&second.lease_id).await.unwrap();
    assert_eq!(first_queue.inflight_len().await.unwrap(), 0);
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_task_queue_drives_worker_when_url_is_configured() {
    let Some(url) = postgres_url_from_env() else {
        eprintln!("skipping postgres queue worker integration test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let now = Utc::now();
    let queue_name = format!("test-worker-{}", Uuid::new_v4());
    let queue = Arc::new(
        PostgresFlowTaskQueue::connect_with_queue(&url, &queue_name)
            .await
            .unwrap(),
    );
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();

    queue
        .enqueue(FlowTask::ResumeDueWaits { now })
        .await
        .unwrap();
    let worker = FlowWorker::new(engine.clone(), queue.clone());
    let outcomes = worker.run_until_idle().await.unwrap();

    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].resumed_waits,
        vec![(run_id.clone(), "sleep".to_string())]
    );
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);

    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
}

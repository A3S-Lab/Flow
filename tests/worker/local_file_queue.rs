use super::*;

#[tokio::test]
async fn local_file_task_queue_persists_pending_tasks_across_instances() {
    let dir = tempfile::tempdir().unwrap();
    let queue = LocalFileFlowTaskQueue::new(dir.path());
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

    let queue = LocalFileFlowTaskQueue::new(dir.path());
    assert_eq!(queue.len().await.unwrap(), 2);
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::DriveRun {
            run_id: "first".to_string()
        })
    );
    assert_eq!(queue.len().await.unwrap(), 1);

    let queue = LocalFileFlowTaskQueue::new(dir.path());
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::DriveRun {
            run_id: "second".to_string()
        })
    );
    assert_eq!(queue.dequeue().await.unwrap(), None);
}

#[tokio::test]
async fn local_file_task_queue_leases_and_acks_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let queue = LocalFileFlowTaskQueue::new(dir.path());
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "leased".to_string(),
        })
        .await
        .unwrap();

    let lease = queue.lease().await.unwrap().unwrap();
    assert_eq!(
        lease.task,
        FlowTask::DriveRun {
            run_id: "leased".to_string()
        }
    );
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    queue.ack(&lease.lease_id).await.unwrap();
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.dequeue().await.unwrap(), None);
}

#[tokio::test]
async fn local_file_task_queue_rejects_absolute_ack_path_without_deleting_external_file() {
    let queue_dir = tempfile::tempdir().unwrap();
    let external_dir = tempfile::tempdir().unwrap();
    let external_path = external_dir.path().join("ack-victim.json");
    tokio::fs::write(&external_path, b"must survive")
        .await
        .unwrap();

    let queue = LocalFileFlowTaskQueue::new(queue_dir.path());
    let malicious_lease_id = external_path.to_string_lossy().into_owned();
    let err = queue.ack(&malicious_lease_id).await.unwrap_err();

    assert!(
        matches!(err, FlowError::LeaseLost(lease_id) if lease_id == malicious_lease_id),
        "absolute paths must be rejected as lost leases"
    );
    assert_eq!(
        tokio::fs::read(&external_path).await.unwrap(),
        b"must survive"
    );
}

#[tokio::test]
async fn local_file_task_queue_rejects_absolute_heartbeat_path_without_moving_external_file() {
    let queue_dir = tempfile::tempdir().unwrap();
    let external_dir = tempfile::tempdir().unwrap();
    let external_path = external_dir.path().join("heartbeat-victim.json");
    tokio::fs::write(&external_path, b"must stay outside")
        .await
        .unwrap();
    tokio::fs::create_dir_all(queue_dir.path().join("inflight"))
        .await
        .unwrap();

    let queue = LocalFileFlowTaskQueue::new(queue_dir.path());
    let malicious_lease_id = external_path.to_string_lossy().into_owned();
    let err = queue.heartbeat(&malicious_lease_id).await.unwrap_err();

    assert!(
        matches!(err, FlowError::LeaseLost(lease_id) if lease_id == malicious_lease_id),
        "absolute paths must be rejected as lost leases"
    );
    assert_eq!(
        tokio::fs::read(&external_path).await.unwrap(),
        b"must stay outside"
    );
}

#[tokio::test]
async fn local_file_task_queue_rejects_parent_traversal_without_deleting_queue_files() {
    let dir = tempfile::tempdir().unwrap();
    tokio::fs::create_dir_all(dir.path().join("inflight"))
        .await
        .unwrap();
    let protected_path = dir.path().join("protected.json");
    tokio::fs::write(&protected_path, b"must survive")
        .await
        .unwrap();

    let queue = LocalFileFlowTaskQueue::new(dir.path());
    let malicious_lease_id = "../protected.json";
    let err = queue.ack(malicious_lease_id).await.unwrap_err();

    assert!(
        matches!(err, FlowError::LeaseLost(lease_id) if lease_id == malicious_lease_id),
        "parent traversal must be rejected as a lost lease"
    );
    assert_eq!(
        tokio::fs::read(&protected_path).await.unwrap(),
        b"must survive"
    );
}

#[tokio::test]
async fn local_file_task_queue_rejects_noncanonical_lease_file_names() {
    let dir = tempfile::tempdir().unwrap();
    let inflight_dir = dir.path().join("inflight");
    tokio::fs::create_dir_all(&inflight_dir).await.unwrap();
    let ack_path = inflight_dir.join("not-a-lease.json");
    let heartbeat_path = inflight_dir.join("also-not-a-lease.json");
    tokio::fs::write(&ack_path, b"ack target").await.unwrap();
    tokio::fs::write(&heartbeat_path, b"heartbeat target")
        .await
        .unwrap();

    let queue = LocalFileFlowTaskQueue::new(dir.path());
    let ack_err = queue.ack("not-a-lease.json").await.unwrap_err();
    let heartbeat_err = queue.heartbeat("also-not-a-lease.json").await.unwrap_err();

    assert!(matches!(ack_err, FlowError::LeaseLost(lease_id) if lease_id == "not-a-lease.json"));
    assert!(
        matches!(heartbeat_err, FlowError::LeaseLost(lease_id) if lease_id == "also-not-a-lease.json")
    );
    assert_eq!(tokio::fs::read(&ack_path).await.unwrap(), b"ack target");
    assert_eq!(
        tokio::fs::read(&heartbeat_path).await.unwrap(),
        b"heartbeat target"
    );
}

#[tokio::test]
async fn local_file_task_queue_requeues_unacked_inflight_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let queue = Arc::new(LocalFileFlowTaskQueue::new(dir.path()));
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "missing-run".to_string(),
        })
        .await
        .unwrap();

    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let worker = FlowWorker::new(engine, queue.clone());
    let err = worker.run_once().await.unwrap_err();
    assert!(matches!(err, FlowError::RunNotFound(run_id) if run_id == "missing-run"));
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    let queue = LocalFileFlowTaskQueue::new(dir.path());
    assert_eq!(queue.requeue_inflight().await.unwrap(), 1);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.len().await.unwrap(), 1);
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::DriveRun {
            run_id: "missing-run".to_string()
        })
    );
}

#[tokio::test]
async fn local_file_task_queue_requeues_expired_inflight_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let queue = LocalFileFlowTaskQueue::new(dir.path());
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "expired-run".to_string(),
        })
        .await
        .unwrap();

    let lease = queue.lease().await.unwrap().unwrap();
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
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.len().await.unwrap(), 1);
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::DriveRun {
            run_id: "expired-run".to_string()
        })
    );
    assert_eq!(queue.dead_letter_len().await.unwrap(), 0);

    let err = queue.ack(&lease.lease_id).await.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(lease_id) if lease_id == lease.lease_id));
}

#[tokio::test]
async fn local_file_task_queue_heartbeat_refreshes_age_and_fences_old_token() {
    let dir = tempfile::tempdir().unwrap();
    let queue = LocalFileFlowTaskQueue::new(dir.path());
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "heartbeat".to_string(),
        })
        .await
        .unwrap();

    let lease = queue.lease().await.unwrap().unwrap();
    let cutoff = Utc::now();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let renewed_lease_id = queue.heartbeat(&lease.lease_id).await.unwrap();
    assert_ne!(renewed_lease_id, lease.lease_id);

    assert_eq!(queue.requeue_inflight_older_than(cutoff).await.unwrap(), 0);
    let err = queue.ack(&lease.lease_id).await.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(lease_id) if lease_id == lease.lease_id));
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    queue.ack(&renewed_lease_id).await.unwrap();
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
}

#[tokio::test]
async fn worker_drops_task_future_after_heartbeat_detects_lease_loss() {
    let runtime = Arc::new(BlockingAfterWaitRuntime {
        started: Notify::new(),
        dropped: Arc::new(AtomicUsize::new(0)),
    });
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({})).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let queue = Arc::new(LocalFileFlowTaskQueue::new(dir.path()));
    queue
        .enqueue(FlowTask::ResumeWait {
            run_id,
            wait_id: "blocked".to_string(),
        })
        .await
        .unwrap();
    let worker = FlowWorker::new(engine, queue.clone())
        .with_heartbeat_interval(Duration::from_millis(10))
        .unwrap();

    let worker_task = tokio::spawn(async move { worker.run_once().await });
    runtime.started.notified().await;
    assert_eq!(queue.requeue_inflight().await.unwrap(), 1);

    let result = tokio::time::timeout(Duration::from_secs(1), worker_task)
        .await
        .expect("worker should observe lease loss")
        .expect("worker task should not panic");
    let err = result.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(_)));
    assert_eq!(runtime.dropped.load(Ordering::SeqCst), 1);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.len().await.unwrap(), 1);
}

#[tokio::test]
async fn local_file_task_queue_dead_letters_expired_inflight_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let queue = LocalFileFlowTaskQueue::new(dir.path());
    let task = FlowTask::DriveRun {
        run_id: "poison-run".to_string(),
    };
    queue.enqueue(task.clone()).await.unwrap();

    let lease = queue.lease().await.unwrap().unwrap();
    assert_eq!(
        queue
            .dead_letter_inflight_older_than(
                Utc::now() - ChronoDuration::seconds(1),
                "lease still fresh",
            )
            .await
            .unwrap(),
        0
    );
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
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.dead_letter_len().await.unwrap(), 1);

    let dead = queue.dead_lettered_tasks().await.unwrap();
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0].lease_id, lease.lease_id);
    assert_eq!(dead[0].task, task);
    assert_eq!(dead[0].reason, "lease expired after worker failure");
}

#[tokio::test]
async fn local_file_task_queue_redrives_a_dead_letter_without_duplication() {
    let dir = tempfile::tempdir().unwrap();
    let queue = LocalFileFlowTaskQueue::new(dir.path());
    let task = FlowTask::DriveRun {
        run_id: "redrive-run".to_string(),
    };
    queue.enqueue(task.clone()).await.unwrap();
    let lease = queue.lease().await.unwrap().unwrap();
    assert_eq!(
        queue
            .dead_letter_inflight_older_than(
                Utc::now() + ChronoDuration::seconds(1),
                "operator redrive test",
            )
            .await
            .unwrap(),
        1
    );

    assert!(queue.redrive_dead_lettered(&lease.lease_id).await.unwrap());
    assert!(!queue.redrive_dead_lettered(&lease.lease_id).await.unwrap());
    assert_eq!(queue.dead_letter_len().await.unwrap(), 0);
    assert_eq!(queue.len().await.unwrap(), 1);
    assert_eq!(queue.dequeue().await.unwrap(), Some(task));
}

#[tokio::test]
async fn local_file_task_queue_drives_worker_after_restart() {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();

    {
        let queue = LocalFileFlowTaskQueue::new(dir.path());
        queue
            .enqueue(FlowTask::ResumeDueWaits { now })
            .await
            .unwrap();
        assert_eq!(queue.len().await.unwrap(), 1);
    }

    let queue = Arc::new(LocalFileFlowTaskQueue::new(dir.path()));
    let worker = FlowWorker::new(engine.clone(), queue.clone());
    let outcomes = worker.run_until_idle().await.unwrap();

    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].resumed_waits,
        vec![(run_id.clone(), "sleep".to_string())]
    );
    assert!(queue.is_empty().await.unwrap());

    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
}

#[cfg(feature = "postgres")]
use a3s_flow::PostgresFlowTaskQueue;
use a3s_flow::{FlowTask, FlowTaskQueue, LocalFileFlowTaskQueue};
use chrono::{DateTime, Utc};
use std::sync::Arc;
#[cfg(feature = "postgres")]
use uuid::Uuid;

fn task(run_id: &str) -> FlowTask {
    FlowTask::DriveRun {
        run_id: run_id.to_string(),
    }
}

#[tokio::test]
async fn local_queue_minimum_cutoff_does_not_panic_or_requeue_a_lease() {
    let directory = tempfile::tempdir().unwrap();
    let queue = Arc::new(LocalFileFlowTaskQueue::new(directory.path()));
    queue.enqueue(task("minimum-cutoff")).await.unwrap();
    let lease = queue.lease().await.unwrap().unwrap();

    let reclaimer = queue.clone();
    let requeued = tokio::spawn(async move {
        reclaimer
            .requeue_inflight_older_than(DateTime::<Utc>::MIN_UTC)
            .await
    })
    .await
    .expect("minimum cutoff handling must not panic")
    .unwrap();

    assert_eq!(requeued, 0);
    assert_eq!(queue.inflight_len().await.unwrap(), 1);
    queue.ack(&lease.lease_id).await.unwrap();
}

#[tokio::test]
async fn local_queue_maximum_cutoff_does_not_panic_and_requeues_every_lease() {
    let directory = tempfile::tempdir().unwrap();
    let queue = Arc::new(LocalFileFlowTaskQueue::new(directory.path()));
    queue.enqueue(task("maximum-cutoff")).await.unwrap();
    queue.lease().await.unwrap().unwrap();

    let reclaimer = queue.clone();
    let requeued = tokio::spawn(async move {
        reclaimer
            .requeue_inflight_older_than(DateTime::<Utc>::MAX_UTC)
            .await
    })
    .await
    .expect("maximum cutoff handling must not panic")
    .unwrap();

    assert_eq!(requeued, 1);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.dequeue().await.unwrap(), Some(task("maximum-cutoff")));
}

#[cfg(feature = "postgres")]
fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_queue_extreme_cutoffs_preserve_ordering_when_url_is_configured() {
    let Some(postgres_url) = postgres_url_from_env() else {
        return;
    };
    let queue_name = format!("time-bounds-{}", Uuid::new_v4());
    let queue = PostgresFlowTaskQueue::connect_with_queue(&postgres_url, queue_name)
        .await
        .unwrap();
    queue.enqueue(task("postgres-cutoffs")).await.unwrap();
    queue.lease().await.unwrap().unwrap();

    assert_eq!(
        queue
            .requeue_inflight_older_than(DateTime::<Utc>::MIN_UTC)
            .await
            .unwrap(),
        0
    );
    assert_eq!(queue.inflight_len().await.unwrap(), 1);
    assert_eq!(
        queue
            .requeue_inflight_older_than(DateTime::<Utc>::MAX_UTC)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(task("postgres-cutoffs"))
    );
}

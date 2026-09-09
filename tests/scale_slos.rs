//! Published scale SLO harness for FLOW-R6 load certification.
//!
//! The in-memory path always runs as an engineering gate and proves the
//! measurement harness. Real-provider assertions against the published SQL
//! targets run only when `A3S_FLOW_POSTGRES_URL` is set.

use a3s_flow::{
    FlowEngine, FlowEvent, FlowEventStore, FlowRuntime, InMemoryEventStore, RuntimeCommand,
    WorkflowInvocation, WorkflowSpec, MAX_FLOW_HISTORY_PAGE_SIZE,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(feature = "postgres")]
use a3s_flow::{migrate_postgres_flow, PostgresEventStore};
#[cfg(feature = "postgres")]
use a3s_orm::PostgresExecutor;
#[cfg(feature = "postgres")]
use std::time::{SystemTime, UNIX_EPOCH};

struct SloRuntime;

#[async_trait]
impl FlowRuntime for SloRuntime {
    async fn run_workflow(
        &self,
        _invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Err(a3s_flow::FlowError::Runtime("unused".into()))
    }

    async fn run_step(
        &self,
        _invocation: a3s_flow::StepInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        Err(a3s_flow::FlowError::Runtime("unused".into()))
    }
}

fn percentile_ns(samples: &mut [u128], percentile: f64) -> u128 {
    assert!(!samples.is_empty());
    samples.sort_unstable();
    let rank = ((percentile / 100.0) * ((samples.len() - 1) as f64)).round() as usize;
    samples[rank.min(samples.len() - 1)]
}

fn assert_duration_budget(label: &str, value: Duration, budget: Duration) {
    assert!(
        value <= budget,
        "{label} observed {value:?} exceeds budget {budget:?}"
    );
}

async fn seed_run(store: &dyn FlowEventStore, run_id: &str, extra_events: usize) {
    store
        .append(
            run_id,
            FlowEvent::RunCreated {
                spec: WorkflowSpec::rust_embedded("cert.slo", "1", "tests::scale_slos", "main"),
                input: json!({ "extra": extra_events }),
            },
        )
        .await
        .unwrap();
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
    for index in 0..extra_events {
        store
            .append(
                run_id,
                FlowEvent::WaitCreated {
                    wait_id: format!("w-{index}"),
                    resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
                },
            )
            .await
            .unwrap();
    }
}

async fn measure_append_latencies(
    store: &dyn FlowEventStore,
    run_id: &str,
    samples: usize,
) -> Vec<u128> {
    let mut tip = store
        .latest_event(run_id)
        .await
        .unwrap()
        .map(|(sequence, _)| sequence)
        .unwrap_or(0);
    let mut latencies = Vec::with_capacity(samples);
    for index in 0..samples {
        let started = Instant::now();
        store
            .append_if_sequence(
                run_id,
                tip,
                FlowEvent::WaitCreated {
                    wait_id: format!("slo-{index}"),
                    resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
                },
            )
            .await
            .unwrap();
        latencies.push(started.elapsed().as_nanos());
        tip += 1;
    }
    latencies
}

async fn measure_page_latencies(
    store: &dyn FlowEventStore,
    run_id: &str,
    samples: usize,
) -> Vec<u128> {
    let mut latencies = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        let page = store
            .list_page(run_id, 0, MAX_FLOW_HISTORY_PAGE_SIZE.min(100))
            .await
            .unwrap();
        latencies.push(started.elapsed().as_nanos());
        assert!(!page.is_empty());
    }
    latencies
}

async fn measure_checkpointed_snapshot_latencies(
    engine: &FlowEngine,
    run_id: &str,
    samples: usize,
) -> Vec<u128> {
    engine.checkpoint(run_id).await.unwrap();
    let mut latencies = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        let snapshot = engine.snapshot(run_id).await.unwrap();
        latencies.push(started.elapsed().as_nanos());
        assert!(snapshot.last_sequence >= 2);
    }
    latencies
}

#[cfg(feature = "postgres")]
fn assert_sql_slo_budgets(mut append: Vec<u128>, mut page: Vec<u128>, mut checkpoint: Vec<u128>) {
    // Published targets from docs/ROADMAP.md for production-capable SQL stores.
    assert_duration_budget(
        "append p50",
        Duration::from_nanos(percentile_ns(&mut append, 50.0) as u64),
        Duration::from_millis(5),
    );
    assert_duration_budget(
        "append p99",
        Duration::from_nanos(percentile_ns(&mut append, 99.0) as u64),
        Duration::from_millis(25),
    );
    assert_duration_budget(
        "page p50",
        Duration::from_nanos(percentile_ns(&mut page, 50.0) as u64),
        Duration::from_millis(5),
    );
    assert_duration_budget(
        "page p99",
        Duration::from_nanos(percentile_ns(&mut page, 99.0) as u64),
        Duration::from_millis(20),
    );
    assert_duration_budget(
        "checkpoint p50",
        Duration::from_nanos(percentile_ns(&mut checkpoint, 50.0) as u64),
        Duration::from_millis(2),
    );
    assert_duration_budget(
        "checkpoint p99",
        Duration::from_nanos(percentile_ns(&mut checkpoint, 99.0) as u64),
        Duration::from_millis(10),
    );
}

#[tokio::test]
async fn in_memory_scale_slo_harness_records_percentiles() {
    let store = Arc::new(InMemoryEventStore::new());
    let run_id = "slo-memory";
    seed_run(store.as_ref(), run_id, 64).await;
    let engine = FlowEngine::new(Arc::clone(&store) as _, Arc::new(SloRuntime));

    let mut append = measure_append_latencies(store.as_ref(), run_id, 64).await;
    let mut page = measure_page_latencies(store.as_ref(), run_id, 64).await;
    let mut checkpoint = measure_checkpointed_snapshot_latencies(&engine, run_id, 64).await;

    // Engineering gate only: the harness must produce finite samples and the
    // in-memory adapter must stay well under the SQL publication budgets.
    assert!(percentile_ns(&mut append, 99.0) > 0);
    assert!(percentile_ns(&mut page, 99.0) > 0);
    assert!(percentile_ns(&mut checkpoint, 99.0) > 0);
    assert_duration_budget(
        "memory append p99",
        Duration::from_nanos(percentile_ns(&mut append, 99.0) as u64),
        Duration::from_millis(5),
    );
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_scale_slos_meet_published_targets_when_url_is_configured() {
    let Ok(url) = std::env::var("A3S_FLOW_POSTGRES_URL") else {
        eprintln!("skipping postgres scale SLOs: A3S_FLOW_POSTGRES_URL is unset");
        return;
    };
    let executor = PostgresExecutor::connect_no_tls(&url, 2).unwrap();
    migrate_postgres_flow(&executor).await.unwrap();
    let store = Arc::new(PostgresEventStore::connect(&url).await.unwrap());
    assert!(store.capabilities().production_ready());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let run_id = format!("slo-pg-{nanos}");
    seed_run(store.as_ref(), &run_id, 128).await;
    let engine = FlowEngine::new(Arc::clone(&store) as _, Arc::new(SloRuntime));

    let append = measure_append_latencies(store.as_ref(), &run_id, 128).await;
    let page = measure_page_latencies(store.as_ref(), &run_id, 128).await;
    let checkpoint = measure_checkpointed_snapshot_latencies(&engine, &run_id, 128).await;
    assert_sql_slo_budgets(append, page, checkpoint);
}

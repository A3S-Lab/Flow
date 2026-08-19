use a3s_flow::{
    FlowEngine, FlowError, FlowEventStore, FlowRuntime, FlowScheduler, FlowTask,
    FlowTaskDispatcher, FlowTaskQueue, FlowWorker, InMemoryEventStore, InMemoryFlowTaskQueue,
    RuntimeBuildCompatibility, RuntimeBuildId, RuntimeBuildTaskRouter, RuntimeCommand,
    StepInvocation, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[path = "runtime_build_routing/cancellation_continuation.rs"]
mod cancellation_continuation;

const TIMER_ID: &str = "runtime-build-timer";

fn timestamp(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}

fn build_id(value: &str) -> RuntimeBuildId {
    RuntimeBuildId::new(value).unwrap()
}

fn pinned_spec(name: &str, build_id: RuntimeBuildId) -> WorkflowSpec {
    WorkflowSpec::rust_embedded(name, "1", "tests::runtime_build_routing", "main")
        .with_runtime_build(build_id)
}

fn build_engine(
    store: Arc<InMemoryEventStore>,
    current: RuntimeBuildId,
    compatible: &[RuntimeBuildId],
) -> FlowEngine {
    let compatibility = compatible.iter().cloned().fold(
        RuntimeBuildCompatibility::new(current),
        RuntimeBuildCompatibility::with_compatible_build,
    );
    FlowEngine::builder(Arc::new(TimerRuntime))
        .with_store(store)
        .with_runtime_build_compatibility(compatibility)
        .build()
}

struct TimerRuntime;

#[async_trait]
impl FlowRuntime for TimerRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.wait_completed(TIMER_ID) {
            return Ok(context.complete(json!({ "build_safe": true })));
        }
        let resume_at = context.input()["resume_at"]
            .as_str()
            .ok_or_else(|| FlowError::Runtime("missing resume_at".to_string()))?
            .parse::<DateTime<Utc>>()
            .map_err(|error| FlowError::Runtime(format!("invalid resume_at: {error}")))?;
        Ok(context.wait_until(TIMER_ID, resume_at))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("timer runtime does not schedule steps")
    }
}

#[derive(Default)]
struct RecordingDispatcher {
    tasks: Mutex<Vec<FlowTask>>,
}

impl RecordingDispatcher {
    fn tasks(&self) -> Vec<FlowTask> {
        self.tasks.lock().unwrap().clone()
    }
}

#[async_trait]
impl FlowTaskDispatcher for RecordingDispatcher {
    async fn dispatch(&self, task: FlowTask) -> a3s_flow::Result<()> {
        self.tasks.lock().unwrap().push(task);
        Ok(())
    }
}

fn assert_runtime_unavailable(
    error: FlowError,
    run_id: &str,
    required: Option<&RuntimeBuildId>,
    current: Option<&RuntimeBuildId>,
) {
    assert!(
        matches!(
            &error,
            FlowError::RuntimeBuildUnavailable {
                run_id: actual_run_id,
                required_build_id,
                current_build_id,
            } if actual_run_id == run_id
                && required_build_id.as_ref() == required
                && current_build_id.as_ref() == current
        ),
        "expected runtime build admission error, got {error:?}"
    );
}

#[test]
fn workflow_spec_runtime_build_identity_is_typed_and_backward_compatible() {
    let legacy: WorkflowSpec = serde_json::from_value(json!({
        "name": "legacy.workflow",
        "version": "1",
        "runtime": {
            "kind": "rust_embedded",
            "entrypoint": "tests::legacy",
            "export_name": "main"
        }
    }))
    .unwrap();
    legacy.validate().unwrap();
    assert_eq!(legacy.runtime_build_id, None);

    let build = build_id("flow-worker-2026.08.09+sha.51e73a2");
    let pinned = pinned_spec("pinned.workflow", build.clone());
    pinned.validate().unwrap();
    let encoded = serde_json::to_value(&pinned).unwrap();
    assert_eq!(encoded["runtime_build_id"], build.as_str());

    for invalid in [
        "",
        " build-a",
        "build a",
        "build\\a",
        "x\nworker",
        "-build-a",
        "build-a/",
    ] {
        assert!(
            RuntimeBuildId::new(invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
}

#[tokio::test]
async fn pinned_runs_require_an_explicitly_compatible_engine_before_history_is_written() {
    let store = Arc::new(InMemoryEventStore::new());
    let required = build_id("worker-v1");
    let spec = pinned_spec("pinned.start", required.clone());
    let input = json!({ "resume_at": "2026-08-09T00:00:00Z" });
    let engine = FlowEngine::new(store.clone(), Arc::new(TimerRuntime));

    let error = engine
        .start_with_id("pinned-start", spec, input)
        .await
        .unwrap_err();

    assert_runtime_unavailable(error, "pinned-start", Some(&required), None);
    assert!(matches!(
        store.list("pinned-start").await,
        Err(FlowError::RunNotFound(_))
    ));
}

#[tokio::test]
async fn configured_engines_reject_unpinned_runs_unless_legacy_admission_is_explicit() {
    let current = build_id("worker-v2");
    let strict_store = Arc::new(InMemoryEventStore::new());
    let strict = FlowEngine::builder(Arc::new(TimerRuntime))
        .with_store(strict_store.clone())
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(current.clone()))
        .build();
    let legacy_spec = WorkflowSpec::rust_embedded(
        "legacy.workflow",
        "1",
        "tests::runtime_build_routing",
        "main",
    );
    let input = json!({ "resume_at": "2026-08-09T00:00:00Z" });

    let error = strict
        .start_with_id("strict-legacy", legacy_spec.clone(), input.clone())
        .await
        .unwrap_err();
    assert_runtime_unavailable(error, "strict-legacy", None, Some(&current));
    assert!(matches!(
        strict_store.list("strict-legacy").await,
        Err(FlowError::RunNotFound(_))
    ));

    let migration = FlowEngine::builder(Arc::new(TimerRuntime))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(current).accept_unpinned())
        .build();
    migration
        .start_with_id("accepted-legacy", legacy_spec, input)
        .await
        .unwrap();
    assert_eq!(
        migration.snapshot("accepted-legacy").await.unwrap().status,
        WorkflowRunStatus::Suspended
    );
}

#[tokio::test]
async fn incompatible_worker_cannot_append_before_a_compatible_replacement_recovers() {
    let store = Arc::new(InMemoryEventStore::new());
    let build_v1 = build_id("worker-v1");
    let build_v2 = build_id("worker-v2");
    let owner = build_engine(store.clone(), build_v1.clone(), &[]);
    owner
        .start_with_id(
            "pinned-recovery",
            pinned_spec("pinned.recovery", build_v1.clone()),
            json!({ "resume_at": "2026-08-09T00:00:00Z" }),
        )
        .await
        .unwrap();
    let before = store.list("pinned-recovery").await.unwrap();

    let incompatible = build_engine(store.clone(), build_v2.clone(), &[]);
    let error = incompatible
        .resume_wait("pinned-recovery", TIMER_ID)
        .await
        .unwrap_err();
    assert_runtime_unavailable(error, "pinned-recovery", Some(&build_v1), Some(&build_v2));
    assert_eq!(store.list("pinned-recovery").await.unwrap(), before);

    let replacement = build_engine(store.clone(), build_v2, &[build_v1]);
    replacement
        .resume_wait("pinned-recovery", TIMER_ID)
        .await
        .unwrap();
    assert_eq!(
        replacement
            .snapshot("pinned-recovery")
            .await
            .unwrap()
            .status,
        WorkflowRunStatus::Completed
    );
}

#[tokio::test]
async fn runtime_build_router_selects_exact_and_unpinned_routes_and_fails_closed() {
    let build_v1 = build_id("worker-v1");
    let build_v2 = build_id("worker-v2");
    let v1 = Arc::new(RecordingDispatcher::default());
    let legacy = Arc::new(RecordingDispatcher::default());
    let router = RuntimeBuildTaskRouter::new()
        .with_route(build_v1.clone(), v1.clone())
        .unwrap()
        .with_unpinned_route(legacy.clone())
        .unwrap();
    let pinned_task = FlowTask::DriveRun {
        run_id: "pinned".to_string(),
    };
    let legacy_task = FlowTask::DriveRun {
        run_id: "legacy".to_string(),
    };

    router
        .dispatch_for_runtime_build(Some(&build_v1), pinned_task.clone())
        .await
        .unwrap();
    router.dispatch(legacy_task.clone()).await.unwrap();

    assert_eq!(v1.tasks(), vec![pinned_task]);
    assert_eq!(legacy.tasks(), vec![legacy_task]);
    let error = router
        .dispatch_for_runtime_build(
            Some(&build_v2),
            FlowTask::DriveRun {
                run_id: "missing-route".to_string(),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::RuntimeBuildRouteNotFound {
            required_build_id: Some(required),
        } if required == build_v2
    ));
}

#[tokio::test]
async fn scheduler_routes_each_pinned_run_to_a_compatible_worker_queue() {
    let now = timestamp("2026-08-09T00:00:05Z");
    let store = Arc::new(InMemoryEventStore::new());
    let build_v1 = build_id("worker-v1");
    let build_v2 = build_id("worker-v2");
    let scheduling_engine = build_engine(
        store.clone(),
        build_v2.clone(),
        std::slice::from_ref(&build_v1),
    );
    for (run_id, build) in [
        ("scheduled-v1", build_v1.clone()),
        ("scheduled-v2", build_v2.clone()),
    ] {
        scheduling_engine
            .start_with_id(
                run_id,
                pinned_spec("pinned.schedule", build),
                json!({
                    "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339(),
                }),
            )
            .await
            .unwrap();
    }

    let v1_queue = Arc::new(InMemoryFlowTaskQueue::new());
    let v2_queue = Arc::new(InMemoryFlowTaskQueue::new());
    let router = Arc::new(
        RuntimeBuildTaskRouter::new()
            .with_route(build_v1.clone(), v1_queue.clone())
            .unwrap()
            .with_route(build_v2.clone(), v2_queue.clone())
            .unwrap(),
    );
    let scheduler = FlowScheduler::new(scheduling_engine, router);

    let tick = scheduler.enqueue_due_work(now).await.unwrap();

    assert_eq!(tick.enqueued_tasks, 2);
    assert_eq!(v1_queue.len().await.unwrap(), 1);
    assert_eq!(v2_queue.len().await.unwrap(), 1);
    let v1_worker = FlowWorker::new(build_engine(store.clone(), build_v1, &[]), v1_queue);
    let v2_worker = FlowWorker::new(build_engine(store.clone(), build_v2, &[]), v2_queue);
    let v1_outcome = v1_worker.run_once().await.unwrap().unwrap();
    let v2_outcome = v2_worker.run_once().await.unwrap().unwrap();
    assert_eq!(v1_outcome.run_ids, vec!["scheduled-v1"]);
    assert_eq!(v2_outcome.run_ids, vec!["scheduled-v2"]);
    assert_eq!(
        build_engine(
            store.clone(),
            build_id("worker-v2"),
            &[build_id("worker-v1")]
        )
        .snapshot("scheduled-v1")
        .await
        .unwrap()
        .status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(
        build_engine(store, build_id("worker-v2"), &[build_id("worker-v1")])
            .snapshot("scheduled-v2")
            .await
            .unwrap()
            .status,
        WorkflowRunStatus::Completed
    );
}

#[tokio::test]
async fn scheduler_preflights_every_build_route_before_enqueuing_any_task() {
    let now = timestamp("2026-08-09T00:00:05Z");
    let store = Arc::new(InMemoryEventStore::new());
    let build_v1 = build_id("worker-v1");
    let build_v2 = build_id("worker-v2");
    let engine = build_engine(store, build_v2.clone(), std::slice::from_ref(&build_v1));
    for (run_id, build) in [
        ("a-routed-run", build_v1.clone()),
        ("b-missing-route", build_v2.clone()),
    ] {
        engine
            .start_with_id(
                run_id,
                pinned_spec("pinned.preflight", build),
                json!({
                    "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339(),
                }),
            )
            .await
            .unwrap();
    }

    let routed_queue = Arc::new(InMemoryFlowTaskQueue::new());
    let router = Arc::new(
        RuntimeBuildTaskRouter::new()
            .with_route(build_v1, routed_queue.clone())
            .unwrap(),
    );
    let scheduler = FlowScheduler::new(engine, router);

    let error = scheduler.enqueue_due_work(now).await.unwrap_err();

    assert!(matches!(
        error,
        FlowError::RuntimeBuildRouteNotFound {
            required_build_id: Some(required),
        } if required == build_v2
    ));
    assert_eq!(routed_queue.len().await.unwrap(), 0);
}

#[tokio::test]
async fn incompatible_worker_leaves_task_unacked_for_compatible_recovery() {
    let now = timestamp("2026-08-09T00:00:05Z");
    let store = Arc::new(InMemoryEventStore::new());
    let build_v1 = build_id("worker-v1");
    let build_v2 = build_id("worker-v2");
    let owner = build_engine(store.clone(), build_v1.clone(), &[]);
    owner
        .start_with_id(
            "worker-recovery",
            pinned_spec("pinned.worker-recovery", build_v1.clone()),
            json!({
                "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339(),
            }),
        )
        .await
        .unwrap();
    let history_before = store.list("worker-recovery").await.unwrap();
    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    queue
        .enqueue(FlowTask::ResumeScheduledRun {
            run_id: "worker-recovery".to_string(),
            now,
        })
        .await
        .unwrap();

    let incompatible = FlowWorker::new(
        build_engine(store.clone(), build_v2.clone(), &[]),
        queue.clone(),
    );
    let error = incompatible.run_once().await.unwrap_err();

    assert_runtime_unavailable(error, "worker-recovery", Some(&build_v1), Some(&build_v2));
    assert_eq!(store.list("worker-recovery").await.unwrap(), history_before);
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    assert_eq!(queue.requeue_inflight().await.unwrap(), 1);
    let compatible = FlowWorker::new(
        build_engine(store.clone(), build_v2, &[build_v1]),
        queue.clone(),
    );
    let outcome = compatible.run_once().await.unwrap().unwrap();

    assert_eq!(outcome.run_ids, vec!["worker-recovery"]);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(
        compatible
            .engine()
            .snapshot("worker-recovery")
            .await
            .unwrap()
            .status,
        WorkflowRunStatus::Completed
    );
}

#[tokio::test]
async fn router_resolves_matching_explicit_run_tasks_and_rejects_ambiguous_targets() {
    let store = Arc::new(InMemoryEventStore::new());
    let build = build_id("worker-v1");
    let engine = build_engine(store, build.clone(), &[]);
    engine
        .start_with_id(
            "routed-run",
            pinned_spec("pinned.direct-route", build.clone()),
            json!({ "resume_at": "2026-08-09T00:00:10Z" }),
        )
        .await
        .unwrap();
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let router = RuntimeBuildTaskRouter::new()
        .with_route(build, dispatcher.clone())
        .unwrap();
    let matching = FlowTask::DriveRun {
        run_id: "routed-run".to_string(),
    };

    router
        .dispatch_for_run(&engine, "routed-run", matching.clone())
        .await
        .unwrap();
    assert_eq!(dispatcher.tasks(), vec![matching]);

    let mismatch = router
        .dispatch_for_run(
            &engine,
            "routed-run",
            FlowTask::DriveRun {
                run_id: "different-run".to_string(),
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(mismatch, FlowError::InvalidTransition(message) if message.contains("different-run"))
    );

    let ambiguous = router
        .dispatch_for_run(
            &engine,
            "routed-run",
            FlowTask::ResumeDueWaits {
                now: timestamp("2026-08-09T00:00:05Z"),
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(ambiguous, FlowError::InvalidTransition(message) if message.contains("explicit run id"))
    );
    assert_eq!(dispatcher.tasks().len(), 1);
}

#[tokio::test]
async fn plain_queues_reject_pinned_scheduler_dispatch_instead_of_misrouting() {
    let now = timestamp("2026-08-09T00:00:05Z");
    let store = Arc::new(InMemoryEventStore::new());
    let build = build_id("worker-v1");
    let engine = build_engine(store, build.clone(), &[]);
    engine
        .start_with_id(
            "missing-router",
            pinned_spec("pinned.schedule", build.clone()),
            json!({
                "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339(),
            }),
        )
        .await
        .unwrap();
    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine, queue.clone());

    let error = scheduler.enqueue_due_work(now).await.unwrap_err();

    assert!(matches!(
        error,
        FlowError::RuntimeBuildRouteNotFound {
            required_build_id: Some(required),
        } if required == build
    ));
    assert_eq!(queue.len().await.unwrap(), 0);
}

use a3s_flow::{
    ChildOperationReference, FlowEngine, FlowEvent, FlowEventStore, FlowRunShardLayout,
    FlowRuntime, InMemoryEventStore, LocalFileEventStore, RuntimeCommand, WorkflowInvocation,
    WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct NoopRuntime;

#[async_trait]
impl FlowRuntime for NoopRuntime {
    async fn run_workflow(
        &self,
        _invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Ok(RuntimeCommand::Complete {
            output: json!({ "ok": true }),
        })
    }

    async fn run_step(
        &self,
        _invocation: a3s_flow::StepInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        Err(a3s_flow::FlowError::Runtime("unused".into()))
    }
}

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("shard.demo", "1", "tests::shard", "main")
}

#[test]
fn layout_routes_runs_stably() {
    let layout = FlowRunShardLayout::new(8).unwrap();
    assert_eq!(layout.shard_index("alpha"), layout.shard_index("alpha"));
    assert!(layout.shard_index("alpha") < 8);
}

#[tokio::test]
async fn in_memory_shards_isolate_histories_and_preserve_cross_shard_links() {
    let store = InMemoryEventStore::with_shard_count(4).unwrap();
    assert!(store.capabilities().physical_run_sharding());
    let layout = store.shard_layout();
    let parent = "shard-parent".to_string();
    let child = (0..10_000)
        .map(|index| format!("shard-child-{index}"))
        .find(|candidate| layout.shard_index(candidate) != layout.shard_index(&parent))
        .expect("expected a child run id on a different shard");
    assert_ne!(layout.shard_index(&parent), layout.shard_index(&child));

    store
        .append(
            &child,
            FlowEvent::RunCreated {
                spec: spec(),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store
        .append(
            &parent,
            FlowEvent::RunCreated {
                spec: spec(),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store.append(&parent, FlowEvent::RunStarted).await.unwrap();
    store
        .append(
            &parent,
            FlowEvent::ChildOperationLinked {
                child: ChildOperationReference::new("op", "ext", "kind")
                    .with_flow_run_id(child.clone()),
            },
        )
        .await
        .unwrap();

    assert_eq!(store.list(&parent).await.unwrap().len(), 3);
    assert_eq!(store.list(&child).await.unwrap().len(), 1);
    let ids = store.list_run_ids().await.unwrap();
    assert!(ids.contains(&parent));
    assert!(ids.contains(&child));
}

#[tokio::test]
async fn local_file_shards_write_under_shard_directories() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalFileEventStore::with_shard_count(dir.path(), 4).unwrap();
    assert!(store.capabilities().physical_run_sharding());
    let layout = store.shard_layout();

    let engine = FlowEngine::new(Arc::new(store.clone()), Arc::new(NoopRuntime));
    let mut occupied = [false; 4];
    for index in 0..24 {
        let run_id = format!("file-run-{index}");
        occupied[layout.shard_index(&run_id) as usize] = true;
        engine
            .start_with_id(&run_id, spec(), json!({ "n": index }))
            .await
            .unwrap();
        let shard_name = layout.shard_name(layout.shard_index(&run_id)).unwrap();
        let path = dir.path().join(shard_name).join(format!("{run_id}.jsonl"));
        assert!(path.is_file(), "missing {}", path.display());
    }
    assert!(occupied.iter().filter(|seen| **seen).count() >= 2);
    assert!(store.list_run_ids().await.unwrap().len() >= 24);
}

#[tokio::test]
async fn unsharded_stores_do_not_advertise_physical_sharding() {
    assert!(!InMemoryEventStore::new()
        .capabilities()
        .physical_run_sharding());
    let dir = tempfile::tempdir().unwrap();
    assert!(!LocalFileEventStore::new(dir.path())
        .capabilities()
        .physical_run_sharding());
}

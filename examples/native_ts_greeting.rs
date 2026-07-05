use a3s_flow::{
    FlowEngine, LocalFileEventStore, NativeTsRuntime, NativeTsRuntimeConfig, WorkflowSpec,
};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let Some(compiler) = std::env::var_os("A3S_FLOW_NATIVE_TS_COMPILER") else {
        println!("native TypeScript compiler not configured; skipping example");
        println!("set A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler to run it");
        return Ok(());
    };

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
        compiler,
        manifest_dir.join("target/a3s-flow-native-ts/artifacts"),
        &manifest_dir,
    )));
    let store = Arc::new(LocalFileEventStore::new(
        manifest_dir.join("target/a3s-flow-native-ts/events"),
    ));
    let engine = FlowEngine::new(store, runtime);
    let spec = WorkflowSpec::native_ts(
        "examples.native-ts-greeting",
        "0.1.0",
        "examples/native-ts/greeting.ts",
        "main",
    );

    let run_id = engine
        .start_with_id("native-ts-greeting-ada", spec, json!({ "name": "Ada" }))
        .await?;
    let snapshot = engine.snapshot(&run_id).await?;

    println!("run_id={}", snapshot.run_id);
    println!("status={:?}", snapshot.status);
    println!(
        "output={}",
        serde_json::to_string_pretty(&snapshot.output).unwrap()
    );
    Ok(())
}

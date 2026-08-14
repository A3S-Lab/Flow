#![cfg(feature = "native-ts")]

use a3s_flow::{
    FlowEngine, InMemoryEventStore, NativeTsCompilerCapabilities, NativeTsDependencyMode,
    NativeTsRuntime, NativeTsRuntimeConfig, WorkflowRunStatus, WorkflowSpec,
    NATIVE_COMPILER_PROTOCOL,
};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command;

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires A3S_FLOW_NATIVE_TS_COMPILER and Bun"]
async fn native_ts_compiler_executes_real_bun_workflow() {
    let compiler = PathBuf::from(
        std::env::var_os("A3S_FLOW_NATIVE_TS_COMPILER")
            .expect("A3S_FLOW_NATIVE_TS_COMPILER must name the compiler under test"),
    );
    let capabilities_output = Command::new(&compiler)
        .arg("capabilities")
        .output()
        .await
        .expect("compiler capabilities command should start");
    assert!(
        capabilities_output.status.success(),
        "compiler capabilities failed: {}",
        String::from_utf8_lossy(&capabilities_output.stderr)
    );
    let capabilities: NativeTsCompilerCapabilities =
        serde_json::from_slice(&capabilities_output.stdout)
            .expect("compiler capabilities should be valid JSON");
    assert_eq!(capabilities.protocol, NATIVE_COMPILER_PROTOCOL);
    assert!(capabilities.dependency_manifest);

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temporary = tempfile::tempdir().expect("temporary artifact root should be created");
    let runtime = Arc::new(
        NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            compiler,
            temporary.path().join("artifacts"),
            &manifest_dir,
        ))
        .with_dependency_mode(NativeTsDependencyMode::CompilerManifest),
    );
    let spec = WorkflowSpec::native_ts(
        "tests.native-ts-bun-smoke",
        "0.1.0",
        "examples/native-ts/greeting.ts",
        "main",
    );

    let cold = runtime
        .preflight(&spec)
        .await
        .expect("cold Bun preflight should compile the workflow");
    assert!(!cold.cache_hit);
    assert!(cold.artifact.is_file());
    #[cfg(windows)]
    assert_eq!(
        cold.artifact.extension().and_then(|value| value.to_str()),
        Some("exe")
    );

    let warm = runtime
        .preflight(&spec)
        .await
        .expect("warm Bun preflight should reuse the workflow artifact");
    assert!(warm.cache_hit);
    assert_eq!(warm.artifact, cold.artifact);
    assert_eq!(warm.source_hash, cold.source_hash);

    let engine = FlowEngine::new(Arc::new(InMemoryEventStore::new()), runtime);
    let run_id = engine
        .start_with_id("native-ts-bun-smoke", spec, json!({ "name": "Ada" }))
        .await
        .expect("real Bun workflow should execute");
    let snapshot = engine
        .snapshot(&run_id)
        .await
        .expect("completed workflow should remain inspectable");

    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.output, Some(json!({ "message": "hello Ada" })));
}

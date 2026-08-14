use a3s_flow::{NativeTsDependencyMode, NativeTsRuntime, NativeTsRuntimeConfig, WorkflowSpec};
use std::path::PathBuf;

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let Some(compiler) = std::env::var_os("A3S_FLOW_NATIVE_TS_COMPILER") else {
        println!("native TypeScript compiler not configured; skipping example");
        println!("set A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler to run it");
        return Ok(());
    };

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::new(
        compiler,
        manifest_dir.join("target/a3s-flow-native-ts/artifacts"),
        &manifest_dir,
    ))
    .with_dependency_mode(NativeTsDependencyMode::CompilerManifest);
    let spec = WorkflowSpec::native_ts(
        "examples.native-ts-greeting",
        "0.1.0",
        "examples/native-ts/greeting.ts",
        "main",
    );

    let preflight = runtime.preflight(&spec).await?;

    println!("entrypoint={}", preflight.entrypoint.display());
    println!("artifact={}", preflight.artifact.display());
    println!("source_hash={}", preflight.source_hash);
    println!("cache_hit={}", preflight.cache_hit);
    Ok(())
}

#[cfg(all(feature = "native-ts", unix))]
mod native_ts_cache_integrity {
    use a3s_flow::{FlowEngine, FlowError, NativeTsRuntime, NativeTsRuntimeConfig, WorkflowSpec};
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    struct Harness {
        _directory: tempfile::TempDir,
        runtime: Arc<NativeTsRuntime>,
        spec: WorkflowSpec,
        compile_log: PathBuf,
        compiler: PathBuf,
        cache_dir: PathBuf,
    }

    fn shell_quote(path: &Path) -> String {
        let raw = path.to_string_lossy();
        format!("'{}'", raw.replace('\'', "'\"'\"'"))
    }

    fn write_executable(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn compile_count(path: &Path) -> usize {
        fs::read_to_string(path).unwrap_or_default().lines().count()
    }

    fn harness() -> Harness {
        let directory = tempfile::tempdir().unwrap();
        let compiler = directory.path().join("native-compiler");
        let compile_log = directory.path().join("compile.log");
        let entrypoint = directory.path().join("workflow.ts");
        let cache_dir = directory.path().join("cache");
        let compiler_source = format!(
            r#"#!/bin/sh
set -eu
printf 'compile\n' >> {compile_log}
cp "$2" "$4"
chmod +x "$4"
"#,
            compile_log = shell_quote(&compile_log),
        );
        write_executable(&compiler, &compiler_source);
        write_executable(
            &entrypoint,
            r#"#!/bin/sh
set -eu
cat >/dev/null
printf '{"protocol":"a3s.flow.native_ts.v1","kind":"workflow","ok":true,"output":{"type":"complete","output":{"marker":"healthy-cache"}}}\n'
"#,
        );

        let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            directory.path(),
        )));
        Harness {
            _directory: directory,
            runtime,
            spec: WorkflowSpec::native_ts("native.workflow", "0.1.0", "workflow.ts", "main"),
            compile_log,
            compiler,
            cache_dir,
        }
    }

    impl Harness {
        fn fresh_runtime(&self) -> Arc<NativeTsRuntime> {
            Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
                &self.compiler,
                &self.cache_dir,
                self._directory.path(),
            )))
        }
    }

    async fn assert_runtime_uses_repaired_artifact(
        harness: &Harness,
        runtime: Arc<NativeTsRuntime>,
    ) {
        let engine = FlowEngine::in_memory(runtime);
        let run_id = engine.start(harness.spec.clone(), json!({})).await.unwrap();
        assert_eq!(
            engine.snapshot(&run_id).await.unwrap().output.unwrap()["marker"],
            "healthy-cache"
        );
    }

    #[tokio::test]
    async fn corrupt_cached_artifact_is_recompiled_before_reuse() {
        let harness = harness();
        let first = harness.runtime.preflight(&harness.spec).await.unwrap();
        assert!(!first.cache_hit);
        assert_eq!(compile_count(&harness.compile_log), 1);

        write_executable(&first.artifact, "#!/bin/sh\nexit 99\n");

        let restarted_runtime = harness.fresh_runtime();
        let repaired = restarted_runtime.preflight(&harness.spec).await.unwrap();
        assert!(
            !repaired.cache_hit,
            "an artifact whose contents changed must be recompiled"
        );
        assert_eq!(repaired.artifact, first.artifact);
        assert_eq!(compile_count(&harness.compile_log), 2);
        assert_runtime_uses_repaired_artifact(&harness, restarted_runtime).await;
    }

    #[tokio::test]
    async fn non_executable_cached_artifact_is_recompiled_before_reuse() {
        let harness = harness();
        let first = harness.runtime.preflight(&harness.spec).await.unwrap();
        assert!(!first.cache_hit);
        assert_eq!(compile_count(&harness.compile_log), 1);

        let mut permissions = fs::metadata(&first.artifact).unwrap().permissions();
        permissions.set_mode(0o644);
        fs::set_permissions(&first.artifact, permissions).unwrap();

        let repaired = harness.runtime.preflight(&harness.spec).await.unwrap();
        assert!(
            !repaired.cache_hit,
            "a non-executable artifact must be recompiled"
        );
        assert_eq!(repaired.artifact, first.artifact);
        assert_eq!(compile_count(&harness.compile_log), 2);
        assert_runtime_uses_repaired_artifact(&harness, harness.runtime.clone()).await;
    }

    #[tokio::test]
    async fn corrupt_integrity_manifest_is_recompiled_before_reuse() {
        let harness = harness();
        let first = harness.runtime.preflight(&harness.spec).await.unwrap();
        let manifest = first.artifact.parent().unwrap().join("manifest.json");
        fs::write(&manifest, b"not valid JSON").unwrap();

        let repaired = harness.runtime.preflight(&harness.spec).await.unwrap();
        assert!(!repaired.cache_hit);
        assert_eq!(repaired.artifact, first.artifact);
        assert_eq!(compile_count(&harness.compile_log), 2);
        assert_runtime_uses_repaired_artifact(&harness, harness.runtime.clone()).await;
    }

    #[tokio::test]
    async fn concurrent_repair_converges_on_one_valid_cache_entry() {
        let harness = harness();
        let first = harness.runtime.preflight(&harness.spec).await.unwrap();
        write_executable(&first.artifact, "#!/bin/sh\nexit 99\n");

        let (first_repair, second_repair) = tokio::join!(
            harness.runtime.preflight(&harness.spec),
            harness.runtime.preflight(&harness.spec),
        );
        let first_repair = first_repair.unwrap();
        let second_repair = second_repair.unwrap();
        assert!(
            !first_repair.cache_hit || !second_repair.cache_hit,
            "at least one concurrent caller must perform the repair"
        );
        assert_eq!(first_repair.artifact, second_repair.artifact);
        assert_eq!(fs::read_dir(&harness.cache_dir).unwrap().count(), 1);
        assert_runtime_uses_repaired_artifact(&harness, harness.runtime.clone()).await;
    }

    #[tokio::test]
    async fn successful_compiler_must_produce_an_executable_artifact() {
        let directory = tempfile::tempdir().unwrap();
        let compiler = directory.path().join("non-executable-compiler");
        let entrypoint = directory.path().join("workflow.ts");
        let cache_dir = directory.path().join("cache");
        write_executable(
            &compiler,
            "#!/bin/sh\nset -eu\nprintf '#!/bin/sh\\nexit 0\\n' > \"$4\"\n",
        );
        write_executable(&entrypoint, "#!/bin/sh\nexit 0\n");
        let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            directory.path(),
        ));
        let spec = WorkflowSpec::native_ts("native.workflow", "0.1.0", "workflow.ts", "main");

        let error = runtime.preflight(&spec).await.unwrap_err();
        assert!(
            matches!(error, FlowError::Runtime(message) if message.contains("is not executable"))
        );
        assert_eq!(fs::read_dir(&cache_dir).unwrap().count(), 0);
    }
}

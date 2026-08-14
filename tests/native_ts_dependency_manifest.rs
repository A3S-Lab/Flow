#[cfg(all(feature = "native-ts", unix))]
mod native_ts_dependency_manifest {
    use a3s_flow::{
        FlowError, NativeTsDependencyMode, NativeTsRuntime, NativeTsRuntimeConfig, WorkflowSpec,
    };
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::time::Duration;

    const BASE_MANIFEST: &str = r#"{"protocol":"a3s.flow.native_ts.dependencies.v1","compilerIdentity":"test-compiler-v1","files":["shared.ts","workflow.ts"]}"#;

    fn native_spec() -> WorkflowSpec {
        WorkflowSpec::native_ts("native.manifest", "0.1.0", "workflow.ts", "main")
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

    fn write_manifest_compiler(
        path: &Path,
        compile_log: &Path,
        mutate_dependency: bool,
        manifest_after_compile: &str,
    ) {
        assert!(!manifest_after_compile.contains('\''));
        let mutation = if mutate_dependency {
            "printf 'changed during compile\\n' >> shared.ts"
        } else {
            ":"
        };
        let content = format!(
            r#"#!/bin/sh
set -eu
case "$1" in
  dependencies)
    if [ -f .manifest-compile-finished ]; then
      printf '%s\n' '{manifest_after_compile}'
    else
      printf '%s\n' '{BASE_MANIFEST}'
    fi
    ;;
  compile)
    printf 'compile\n' >> {compile_log}
    if [ "$3" != "-o" ]; then
      echo "expected -o" >&2
      exit 2
    fi
    {mutation}
    : > .manifest-compile-finished
    cp "$2" "$4"
    chmod +x "$4"
    ;;
  *)
    echo "expected dependencies or compile command" >&2
    exit 2
    ;;
esac
"#,
            compile_log = shell_quote(compile_log),
        );
        write_executable(path, &content);
    }

    fn runtime(root: &Path, compiler: &Path, cache: &Path) -> NativeTsRuntime {
        NativeTsRuntime::new(NativeTsRuntimeConfig::new(compiler, cache, root))
            .with_dependency_mode(NativeTsDependencyMode::CompilerManifest)
    }

    fn compile_count(path: &Path) -> usize {
        fs::read_to_string(path).unwrap_or_default().lines().count()
    }

    async fn assert_directory_becomes_empty(path: &Path) {
        let empty = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if fs::read_dir(path).unwrap().next().is_none() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok();
        assert!(
            empty,
            "rejected manifest drift must remove temporary artifacts"
        );
    }

    fn write_sources(root: &Path) {
        fs::write(
            root.join("workflow.ts"),
            "export { value } from './shared';\n",
        )
        .unwrap();
        fs::write(root.join("shared.ts"), "export const value = 1;\n").unwrap();
    }

    #[tokio::test]
    async fn manifest_mode_invalidates_imported_source_changes() {
        let directory = tempfile::tempdir().unwrap();
        let compiler = directory.path().join("manifest-compiler");
        let compile_log = directory.path().join("compile.log");
        let cache = directory.path().join("cache");
        write_sources(directory.path());
        write_manifest_compiler(&compiler, &compile_log, false, BASE_MANIFEST);
        let runtime = runtime(directory.path(), &compiler, &cache);

        let first = runtime.preflight(&native_spec()).await.unwrap();
        let second = runtime.preflight(&native_spec()).await.unwrap();
        assert!(!first.cache_hit);
        assert!(second.cache_hit);
        assert_eq!(second.source_hash, first.source_hash);
        assert_eq!(compile_count(&compile_log), 1);

        fs::write(
            directory.path().join("shared.ts"),
            "export const value = 2;\n",
        )
        .unwrap();
        let third = runtime.preflight(&native_spec()).await.unwrap();

        assert!(!third.cache_hit);
        assert_ne!(third.source_hash, first.source_hash);
        assert_ne!(third.artifact, first.artifact);
        assert_eq!(compile_count(&compile_log), 2);
    }

    #[tokio::test]
    async fn manifest_mode_rejects_dependency_content_drift_during_compile() {
        let directory = tempfile::tempdir().unwrap();
        let compiler = directory.path().join("mutating-compiler");
        let compile_log = directory.path().join("compile.log");
        let cache = directory.path().join("cache");
        write_sources(directory.path());
        write_manifest_compiler(&compiler, &compile_log, true, BASE_MANIFEST);

        let error = runtime(directory.path(), &compiler, &cache)
            .preflight(&native_spec())
            .await
            .unwrap_err();

        assert!(
            matches!(error, FlowError::Runtime(message) if message.contains("changed while it was being compiled"))
        );
        assert_eq!(compile_count(&compile_log), 1);
        assert_directory_becomes_empty(&cache).await;
    }

    #[tokio::test]
    async fn manifest_mode_rejects_dependency_graph_drift_during_compile() {
        let directory = tempfile::tempdir().unwrap();
        let compiler = directory.path().join("graph-drift-compiler");
        let compile_log = directory.path().join("compile.log");
        let cache = directory.path().join("cache");
        write_sources(directory.path());
        fs::write(
            directory.path().join("extra.ts"),
            "export const extra = 1;\n",
        )
        .unwrap();
        let manifest = r#"{"protocol":"a3s.flow.native_ts.dependencies.v1","compilerIdentity":"test-compiler-v1","files":["extra.ts","shared.ts","workflow.ts"]}"#;
        write_manifest_compiler(&compiler, &compile_log, false, manifest);

        let error = runtime(directory.path(), &compiler, &cache)
            .preflight(&native_spec())
            .await
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("changed while it was being compiled"));
        assert_directory_becomes_empty(&cache).await;
    }

    #[tokio::test]
    async fn manifest_mode_rejects_compiler_identity_drift_during_compile() {
        let directory = tempfile::tempdir().unwrap();
        let compiler = directory.path().join("identity-drift-compiler");
        let compile_log = directory.path().join("compile.log");
        let cache = directory.path().join("cache");
        write_sources(directory.path());
        let manifest = r#"{"protocol":"a3s.flow.native_ts.dependencies.v1","compilerIdentity":"test-compiler-v2","files":["shared.ts","workflow.ts"]}"#;
        write_manifest_compiler(&compiler, &compile_log, false, manifest);

        let error = runtime(directory.path(), &compiler, &cache)
            .preflight(&native_spec())
            .await
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("changed while it was being compiled"));
        assert_directory_becomes_empty(&cache).await;
    }
}

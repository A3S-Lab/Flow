#[cfg(all(feature = "native-ts", unix))]
mod native_ts_cache_identity {
    use a3s_flow::{FlowEngine, NativeTsRuntime, NativeTsRuntimeConfig, WorkflowSpec};
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::Arc;

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

    fn write_rewriting_compiler(path: &Path, compile_log: &Path, marker: &str) {
        assert!(
            marker
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-'),
            "test compiler marker must be safe for sed replacement"
        );
        let content = format!(
            r#"#!/bin/sh
set -eu
printf 'compile\n' >> {compile_log}
sed 's/native-cache-compiler-marker/{marker}/g' "$2" > "$4"
chmod +x "$4"
"#,
            compile_log = shell_quote(compile_log),
        );
        write_executable(path, &content);
    }

    fn write_external_input_compiler(path: &Path, compile_log: &Path, external_input: &Path) {
        let content = format!(
            r#"#!/bin/sh
set -eu
printf 'compile\n' >> {compile_log}
marker=$(sed -n '1p' {external_input})
sed "s/native-cache-compiler-marker/$marker/g" "$2" > "$4"
chmod +x "$4"
"#,
            compile_log = shell_quote(compile_log),
            external_input = shell_quote(external_input),
        );
        write_executable(path, &content);
    }

    fn write_runtime_source(path: &Path) {
        write_executable(
            path,
            r#"#!/bin/sh
set -eu
cat >/dev/null
printf '{"protocol":"a3s.flow.native_ts.v1","kind":"workflow","ok":true,"output":{"type":"complete","output":{"marker":"native-cache-compiler-marker"}}}\n'
"#,
        );
    }

    fn compile_count(path: &Path) -> usize {
        fs::read_to_string(path).unwrap_or_default().lines().count()
    }

    #[tokio::test]
    async fn replacing_compiler_at_same_path_invalidates_cached_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("native-compiler");
        let compile_log = dir.path().join("compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let cache_dir = dir.path().join("cache");
        let spec = WorkflowSpec::native_ts("native.workflow", "0.1.0", "workflow.ts", "main");

        write_runtime_source(&entrypoint);
        write_rewriting_compiler(&compiler, &compile_log, "compiler-a");

        let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        )));
        let first = runtime.preflight(&spec).await.unwrap();
        assert!(!first.cache_hit);
        assert_eq!(compile_count(&compile_log), 1);

        write_rewriting_compiler(&compiler, &compile_log, "compiler-b");

        let second = runtime.preflight(&spec).await.unwrap();
        assert!(
            !second.cache_hit,
            "replacing a compiler in place must not reuse its previous artifact"
        );
        assert_ne!(second.artifact, first.artifact);
        assert_eq!(second.source_hash, first.source_hash);
        assert_eq!(compile_count(&compile_log), 2);

        let engine = FlowEngine::in_memory(runtime);
        let run_id = engine.start(spec, json!({})).await.unwrap();
        assert_eq!(
            engine.snapshot(&run_id).await.unwrap().output.unwrap()["marker"],
            "compiler-b"
        );
    }

    #[tokio::test]
    async fn external_compiler_inputs_require_workflow_version_bumps() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("native-compiler");
        let compile_log = dir.path().join("compile.log");
        let external_input = dir.path().join("compiler-input.txt");
        let entrypoint = dir.path().join("workflow.ts");
        let cache_dir = dir.path().join("cache");
        let initial_spec =
            WorkflowSpec::native_ts("native.workflow", "0.1.0", "workflow.ts", "main");

        write_runtime_source(&entrypoint);
        fs::write(&external_input, "dependency-a\n").unwrap();
        write_external_input_compiler(&compiler, &compile_log, &external_input);

        let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        )));
        let first = runtime.preflight(&initial_spec).await.unwrap();
        assert!(!first.cache_hit);
        assert_eq!(compile_count(&compile_log), 1);
        assert!(fs::read_to_string(&first.artifact)
            .unwrap()
            .contains("dependency-a"));

        fs::write(&external_input, "dependency-b\n").unwrap();

        let unchanged_version = runtime.preflight(&initial_spec).await.unwrap();
        assert!(
            unchanged_version.cache_hit,
            "external compiler inputs are deployment-owned and do not invalidate the cache"
        );
        assert_eq!(unchanged_version.artifact, first.artifact);
        assert_eq!(unchanged_version.source_hash, first.source_hash);
        assert_eq!(compile_count(&compile_log), 1);

        let updated_spec =
            WorkflowSpec::native_ts("native.workflow", "0.1.1", "workflow.ts", "main");
        let updated = runtime.preflight(&updated_spec).await.unwrap();
        assert!(
            !updated.cache_hit,
            "bumping the workflow version must select a new compiled artifact"
        );
        assert_ne!(updated.artifact, first.artifact);
        assert_ne!(updated.source_hash, first.source_hash);
        assert_eq!(compile_count(&compile_log), 2);
        assert!(fs::read_to_string(&updated.artifact)
            .unwrap()
            .contains("dependency-b"));
    }
}

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
}

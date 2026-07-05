#[cfg(all(feature = "native-ts", unix))]
mod native_ts_runtime {
    use a3s_flow::{
        FlowEngine, FlowError, NativeTsRuntime, NativeTsRuntimeConfig, WorkflowRunStatus,
        WorkflowSpec,
    };
    use serde_json::{json, Value};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::Arc;

    fn native_spec(entrypoint: &str) -> WorkflowSpec {
        WorkflowSpec::native_ts("native.workflow", "0.1.0", entrypoint, "main")
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

    fn write_fake_compiler(path: &Path, compile_log: &Path) {
        let content = format!(
            r#"#!/bin/sh
set -eu
printf 'compile\n' >> {compile_log}
if [ "$1" != "compile" ]; then
  echo "expected compile command" >&2
  exit 2
fi
if [ "$3" != "-o" ]; then
  echo "expected -o" >&2
  exit 2
fi
cp "$2" "$4"
chmod +x "$4"
"#,
            compile_log = shell_quote(compile_log),
        );
        write_executable(path, &content);
    }

    fn write_failing_compiler(path: &Path, compile_log: &Path) {
        let content = format!(
            r#"#!/bin/sh
set -eu
printf 'compile\n' >> {compile_log}
echo "compile broke on purpose" >&2
exit 9
"#,
            compile_log = shell_quote(compile_log),
        );
        write_executable(path, &content);
    }

    fn write_runtime_source(path: &Path, request_log: &Path, marker: &str, protocol: &str) {
        let content = format!(
            r#"#!/bin/sh
set -eu
request="$(cat)"
printf '%s\n' "$request" >> {request_log}
printf '{{"protocol":"{protocol}","kind":"workflow","ok":true,"output":{{"type":"complete","output":{{"marker":"{marker}"}}}}}}\n'
"#,
            marker = marker,
            protocol = protocol,
            request_log = shell_quote(request_log),
        );
        write_executable(path, &content);
    }

    fn write_step_runtime_source(path: &Path, request_log: &Path) {
        let content = format!(
            r#"#!/bin/sh
set -eu
request="$(cat)"
printf '%s\n' "$request" >> {request_log}
case "$request" in
  *'"kind":"step"'*)
    printf '{{"protocol":"a3s.flow.native_ts.v1","kind":"step","ok":true,"output":{{"message":"native step complete"}}}}\n'
    ;;
  *'"type":"step_completed"'*)
    printf '{{"protocol":"a3s.flow.native_ts.v1","kind":"workflow","ok":true,"output":{{"type":"complete","output":{{"status":"done"}}}}}}\n'
    ;;
  *)
    printf '{{"protocol":"a3s.flow.native_ts.v1","kind":"workflow","ok":true,"output":{{"type":"schedule_step","step_id":"native-step","step_name":"nativeStep","input":{{"value":42}},"retry":{{"max_attempts":1,"delay_ms":0}}}}}}\n'
    ;;
esac
"#,
            request_log = shell_quote(request_log),
        );
        write_executable(path, &content);
    }

    fn write_mismatched_kind_runtime_source(path: &Path, request_log: &Path) {
        let content = format!(
            r#"#!/bin/sh
set -eu
request="$(cat)"
printf '%s\n' "$request" >> {request_log}
printf '{{"protocol":"a3s.flow.native_ts.v1","kind":"step","ok":true,"output":{{"type":"complete","output":{{}}}}}}\n'
"#,
            request_log = shell_quote(request_log),
        );
        write_executable(path, &content);
    }

    fn write_error_runtime_source(path: &Path, request_log: &Path) {
        let content = format!(
            r#"#!/bin/sh
set -eu
request="$(cat)"
printf '%s\n' "$request" >> {request_log}
printf '{{"protocol":"a3s.flow.native_ts.v1","kind":"workflow","ok":false,"error":"runtime rejected workflow"}}\n'
"#,
            request_log = shell_quote(request_log),
        );
        write_executable(path, &content);
    }

    fn compile_count(path: &Path) -> usize {
        fs::read_to_string(path).unwrap_or_default().lines().count()
    }

    fn requests(path: &Path) -> Vec<Value> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn last_request(path: &Path) -> Value {
        let content = fs::read_to_string(path).unwrap();
        let line = content.lines().last().unwrap();
        serde_json::from_str(line).unwrap()
    }

    #[tokio::test]
    async fn native_runtime_preflight_compiles_and_reports_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("fake-compiler");
        let compile_log = dir.path().join("compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let request_log = dir.path().join("requests.jsonl");
        let cache_dir = dir.path().join("cache");

        write_fake_compiler(&compiler, &compile_log);
        write_runtime_source(
            &entrypoint,
            &request_log,
            "preflight",
            "a3s.flow.native_ts.v1",
        );

        let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        ));
        let spec = native_spec("workflow.ts");

        let first = runtime.preflight(&spec).await.unwrap();
        assert_eq!(first.entrypoint, entrypoint);
        assert!(first.artifact.starts_with(&cache_dir));
        assert_eq!(first.source_hash.len(), 64);
        assert!(!first.cache_hit);
        assert!(first.artifact.is_file());
        assert_eq!(compile_count(&compile_log), 1);

        let second = runtime.preflight(&spec).await.unwrap();
        assert_eq!(second.entrypoint, first.entrypoint);
        assert_eq!(second.artifact, first.artifact);
        assert_eq!(second.source_hash, first.source_hash);
        assert!(second.cache_hit);
        assert_eq!(compile_count(&compile_log), 1);
    }

    #[tokio::test]
    async fn native_runtime_preflight_surfaces_compile_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("failing-compiler");
        let compile_log = dir.path().join("compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let cache_dir = dir.path().join("cache");

        write_failing_compiler(&compiler, &compile_log);
        fs::write(&entrypoint, "export async function main() {}\n").unwrap();

        let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        ));
        let err = runtime
            .preflight(&native_spec("workflow.ts"))
            .await
            .unwrap_err();

        assert!(
            matches!(err, FlowError::Runtime(message) if message.contains("native TypeScript compile failed") && message.contains("compile broke on purpose"))
        );
        assert_eq!(compile_count(&compile_log), 1);
    }

    #[tokio::test]
    async fn native_runtime_preflight_rejects_non_native_ts_spec() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("fake-compiler");
        let compile_log = dir.path().join("compile.log");
        let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            dir.path().join("cache"),
            dir.path(),
        ));
        write_fake_compiler(&compiler, &compile_log);

        let spec = WorkflowSpec::rust_embedded("rust.workflow", "0.1.0", "src/lib.rs", "main");
        let err = runtime.preflight(&spec).await.unwrap_err();

        assert!(
            matches!(err, FlowError::InvalidWorkflow(message) if message.contains("native_ts workflow spec"))
        );
        assert_eq!(compile_count(&compile_log), 0);
    }

    #[tokio::test]
    async fn native_runtime_compiles_by_source_hash_and_reuses_cached_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("fake-compiler");
        let compile_log = dir.path().join("compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let request_log = dir.path().join("requests.jsonl");
        let cache_dir = dir.path().join("cache");

        write_fake_compiler(&compiler, &compile_log);
        write_runtime_source(&entrypoint, &request_log, "first", "a3s.flow.native_ts.v1");

        let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        )));
        let engine = FlowEngine::in_memory(runtime);
        let spec = native_spec("workflow.ts");

        let first_run_id = engine.start(spec.clone(), json!({ "n": 1 })).await.unwrap();
        let first = engine.snapshot(&first_run_id).await.unwrap();
        assert_eq!(first.status, WorkflowRunStatus::Completed);
        assert_eq!(first.output.unwrap()["marker"], "first");
        assert_eq!(compile_count(&compile_log), 1);

        let second_run_id = engine.start(spec.clone(), json!({ "n": 2 })).await.unwrap();
        let second = engine.snapshot(&second_run_id).await.unwrap();
        assert_eq!(second.output.unwrap()["marker"], "first");
        assert_eq!(
            compile_count(&compile_log),
            1,
            "unchanged source should reuse the compiled artifact"
        );

        let request = last_request(&request_log);
        assert_eq!(request["protocol"], "a3s.flow.native_ts.v1");
        assert_eq!(request["kind"], "workflow");
        assert_eq!(request["exportName"], "main");
        assert_eq!(request["payload"]["run_id"], second_run_id);
        assert_eq!(request["sourceHash"].as_str().unwrap().len(), 64);

        write_runtime_source(&entrypoint, &request_log, "second", "a3s.flow.native_ts.v1");

        let third_run_id = engine.start(spec, json!({ "n": 3 })).await.unwrap();
        let third = engine.snapshot(&third_run_id).await.unwrap();
        assert_eq!(third.output.unwrap()["marker"], "second");
        assert_eq!(
            compile_count(&compile_log),
            2,
            "changed source should compile to a new artifact"
        );
    }

    #[tokio::test]
    async fn native_runtime_rejects_invalid_protocol_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("fake-compiler");
        let compile_log = dir.path().join("compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let request_log = dir.path().join("requests.jsonl");
        let cache_dir = dir.path().join("cache");

        write_fake_compiler(&compiler, &compile_log);
        write_runtime_source(&entrypoint, &request_log, "bad", "wrong.protocol");

        let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        )));
        let engine = FlowEngine::in_memory(runtime);

        let err = engine
            .start(native_spec("workflow.ts"), json!({}))
            .await
            .unwrap_err();

        assert!(
            matches!(err, FlowError::Runtime(message) if message.contains("protocol mismatch"))
        );
    }

    #[tokio::test]
    async fn native_runtime_invokes_step_with_same_protocol_and_source_hash() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("fake-compiler");
        let compile_log = dir.path().join("compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let request_log = dir.path().join("requests.jsonl");
        let cache_dir = dir.path().join("cache");

        write_fake_compiler(&compiler, &compile_log);
        write_step_runtime_source(&entrypoint, &request_log);

        let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        )));
        let engine = FlowEngine::in_memory(runtime);
        let run_id = engine
            .start(native_spec("workflow.ts"), json!({ "n": 1 }))
            .await
            .unwrap();
        let snapshot = engine.snapshot(&run_id).await.unwrap();

        assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
        assert_eq!(snapshot.output.unwrap()["status"], "done");
        assert_eq!(
            snapshot.steps["native-step"].output.as_ref().unwrap()["message"],
            "native step complete"
        );
        assert_eq!(compile_count(&compile_log), 1);

        let requests = requests(&request_log);
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0]["kind"], "workflow");
        assert_eq!(requests[1]["kind"], "step");
        assert_eq!(requests[2]["kind"], "workflow");
        assert_eq!(requests[1]["exportName"], "main");
        assert_eq!(requests[1]["payload"]["run_id"], run_id);
        assert_eq!(requests[1]["payload"]["step_id"], "native-step");
        assert_eq!(requests[1]["payload"]["step_name"], "nativeStep");
        assert_eq!(requests[1]["payload"]["input"]["value"], 42);
        assert_eq!(requests[1]["sourceHash"], requests[0]["sourceHash"]);
        assert_eq!(requests[2]["sourceHash"], requests[0]["sourceHash"]);
    }

    #[tokio::test]
    async fn native_runtime_rejects_response_kind_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("fake-compiler");
        let compile_log = dir.path().join("compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let request_log = dir.path().join("requests.jsonl");
        let cache_dir = dir.path().join("cache");

        write_fake_compiler(&compiler, &compile_log);
        write_mismatched_kind_runtime_source(&entrypoint, &request_log);

        let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        )));
        let engine = FlowEngine::in_memory(runtime);

        let err = engine
            .start(native_spec("workflow.ts"), json!({}))
            .await
            .unwrap_err();

        assert!(
            matches!(err, FlowError::Runtime(message) if message.contains("response kind mismatch"))
        );
    }

    #[tokio::test]
    async fn native_runtime_surfaces_error_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("fake-compiler");
        let compile_log = dir.path().join("compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let request_log = dir.path().join("requests.jsonl");
        let cache_dir = dir.path().join("cache");

        write_fake_compiler(&compiler, &compile_log);
        write_error_runtime_source(&entrypoint, &request_log);

        let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        )));
        let engine = FlowEngine::in_memory(runtime);

        let err = engine
            .start(native_spec("workflow.ts"), json!({}))
            .await
            .unwrap_err();

        assert!(
            matches!(err, FlowError::Runtime(message) if message == "runtime rejected workflow")
        );
    }
}

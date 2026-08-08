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
    use std::process::{Command as StdCommand, Stdio};
    use std::sync::Arc;
    use std::time::Duration;

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

    #[derive(Clone, Copy)]
    enum TestOutputStream {
        Stdout,
        Stderr,
    }

    impl TestOutputStream {
        fn name(self) -> &'static str {
            match self {
                Self::Stdout => "stdout",
                Self::Stderr => "stderr",
            }
        }

        fn shell_redirect(self) -> &'static str {
            match self {
                Self::Stdout => "",
                Self::Stderr => ">&2",
            }
        }
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
if [ "$1" != "compile" ]; then
  echo "expected compile command" >&2
  exit 2
fi
if [ "$3" != "-o" ]; then
  echo "expected -o" >&2
  exit 2
fi
sed 's/native-cache-compiler-marker/{marker}/g' "$2" > "$4"
chmod +x "$4"
"#,
            compile_log = shell_quote(compile_log),
        );
        write_executable(path, &content);
    }

    fn write_slow_fake_compiler(path: &Path, compile_log: &Path, started_log: &Path) {
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
printf '#!/bin/sh\n' > "$4"
printf 'started\n' >> {started_log}
sleep 1
cp "$2" "$4"
chmod +x "$4"
"#,
            compile_log = shell_quote(compile_log),
            started_log = shell_quote(started_log),
        );
        write_executable(path, &content);
    }

    fn write_failing_compiler(path: &Path, compile_log: &Path) {
        let content = format!(
            r#"#!/bin/sh
set -eu
printf 'compile\n' >> {compile_log}
if [ "$3" != "-o" ]; then
  echo "expected -o" >&2
  exit 2
fi
printf 'partial artifact\n' > "$4"
echo "compile broke on purpose" >&2
exit 9
"#,
            compile_log = shell_quote(compile_log),
        );
        write_executable(path, &content);
    }

    fn write_blocking_compiler(path: &Path, pid_file: &Path) {
        let content = format!(
            r#"#!/bin/sh
set -eu
printf 'partial artifact\n' > "$4"
printf '%s\n' "$$" > {pid_file}
exec sleep 30
"#,
            pid_file = shell_quote(pid_file),
        );
        write_executable(path, &content);
    }

    fn write_oversized_output_compiler(path: &Path, pid_file: &Path, stream: TestOutputStream) {
        let content = format!(
            r#"#!/bin/sh
set -eu
printf '%s\n' "$$" > {pid_file}
index=0
while [ "$index" -lt 32 ]; do
  printf '0123456789abcdef' {redirect}
  index=$((index + 1))
done
exec sleep 30
"#,
            pid_file = shell_quote(pid_file),
            redirect = stream.shell_redirect(),
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

    fn write_blocking_runtime_source(path: &Path, pid_file: &Path) {
        let content = format!(
            r#"#!/bin/sh
set -eu
printf '%s\n' "$$" > {pid_file}
exec sleep 30
"#,
            pid_file = shell_quote(pid_file),
        );
        write_executable(path, &content);
    }

    fn write_oversized_output_runtime_source(
        path: &Path,
        pid_file: &Path,
        stream: TestOutputStream,
    ) {
        let content = format!(
            r#"#!/bin/sh
set -eu
cat >/dev/null
printf '%s\n' "$$" > {pid_file}
index=0
while [ "$index" -lt 32 ]; do
  printf '0123456789abcdef' {redirect}
  index=$((index + 1))
done
exec sleep 30
"#,
            pid_file = shell_quote(pid_file),
            redirect = stream.shell_redirect(),
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

    async fn wait_for_file(path: &Path) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !path.is_file() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    async fn wait_for_pid(path: &Path) -> Option<u32> {
        let appeared = tokio::time::timeout(Duration::from_secs(15), async {
            while !path.is_file() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok();
        if !appeared {
            return None;
        }
        fs::read_to_string(path).ok()?.trim().parse().ok()
    }

    fn process_is_running(pid: u32) -> bool {
        StdCommand::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn force_kill(pid: u32) {
        let _ = StdCommand::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    async fn assert_process_stops_after_abort(pid: u32, process_kind: &str) {
        let stopped = tokio::time::timeout(Duration::from_secs(2), async {
            while process_is_running(pid) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok();

        if !stopped {
            force_kill(pid);
        }
        assert!(
            stopped,
            "aborting a Native TypeScript future must stop its {process_kind} child process"
        );
    }

    async fn assert_directory_becomes_empty(path: &Path, context: &str) {
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
        assert!(empty, "{context} must remove temporary cache artifacts");
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
    async fn native_runtime_resolves_relative_working_and_cache_paths_once() {
        let current_dir = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir_in(&current_dir).unwrap();
        let relative_working_dir = dir.path().strip_prefix(&current_dir).unwrap();
        let relative_cache_dir = relative_working_dir.join("cache");
        let compiler = dir.path().join("fake-compiler");
        let compile_log = dir.path().join("compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let request_log = dir.path().join("requests.jsonl");
        let cache_dir = dir.path().join("cache");

        write_fake_compiler(&compiler, &compile_log);
        write_runtime_source(
            &entrypoint,
            &request_log,
            "relative-paths",
            "a3s.flow.native_ts.v1",
        );

        let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            relative_working_dir.join("fake-compiler"),
            relative_cache_dir,
            relative_working_dir,
        )));
        let spec = native_spec("workflow.ts");

        let preflight = runtime.preflight(&spec).await.unwrap();

        assert_eq!(preflight.entrypoint, entrypoint);
        assert!(preflight.artifact.starts_with(cache_dir));
        assert!(preflight.artifact.is_file());

        let engine = FlowEngine::in_memory(runtime);
        let run_id = engine.start(spec, json!({})).await.unwrap();
        let snapshot = engine.snapshot(&run_id).await.unwrap();
        assert_eq!(snapshot.output.unwrap()["marker"], "relative-paths");
        assert_eq!(compile_count(&compile_log), 1);
    }

    #[tokio::test]
    async fn native_runtime_does_not_cache_a_partial_concurrent_compile() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("slow-compiler");
        let compile_log = dir.path().join("compile.log");
        let started_log = dir.path().join("started.log");
        let entrypoint = dir.path().join("workflow.ts");
        let request_log = dir.path().join("requests.jsonl");
        let cache_dir = dir.path().join("cache");

        write_slow_fake_compiler(&compiler, &compile_log, &started_log);
        write_runtime_source(
            &entrypoint,
            &request_log,
            "atomic-cache",
            "a3s.flow.native_ts.v1",
        );

        let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        )));
        let spec = native_spec("workflow.ts");
        let first_runtime = Arc::clone(&runtime);
        let first_spec = spec.clone();
        let first = tokio::spawn(async move { first_runtime.preflight(&first_spec).await });

        wait_for_file(&started_log).await;
        let second = runtime.preflight(&spec).await.unwrap();
        let first = first.await.unwrap().unwrap();

        assert!(!first.cache_hit);
        assert!(
            !second.cache_hit,
            "a partially written compiler output must not be visible as a cache hit"
        );
        assert_eq!(first.artifact, second.artifact);
        assert_eq!(compile_count(&compile_log), 2);
        assert_eq!(
            fs::read_dir(&cache_dir).unwrap().count(),
            1,
            "only the atomically published artifact may remain in the cache"
        );

        let engine = FlowEngine::in_memory(runtime);
        let run_id = engine.start(spec, json!({})).await.unwrap();
        let snapshot = engine.snapshot(&run_id).await.unwrap();
        assert_eq!(snapshot.output.unwrap()["marker"], "atomic-cache");
        assert_eq!(compile_count(&compile_log), 2);
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
        assert_eq!(fs::read_dir(&cache_dir).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn native_runtime_preflight_bounds_compiler_output_streams() {
        for stream in [TestOutputStream::Stdout, TestOutputStream::Stderr] {
            let dir = tempfile::tempdir().unwrap();
            let compiler = dir.path().join("oversized-output-compiler");
            let compiler_pid = dir.path().join("compiler.pid");
            let entrypoint = dir.path().join("workflow.ts");
            let cache_dir = dir.path().join("cache");

            write_oversized_output_compiler(&compiler, &compiler_pid, stream);
            fs::write(&entrypoint, "export async function main() {}\n").unwrap();

            let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::new(
                &compiler,
                &cache_dir,
                dir.path(),
            ))
            .with_output_limits(128, 128);
            let error = tokio::time::timeout(
                Duration::from_secs(5),
                runtime.preflight(&native_spec("workflow.ts")),
            )
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "compiler {} output limit must fail before the child finishes",
                    stream.name()
                )
            })
            .unwrap_err();

            let message = match error {
                FlowError::Runtime(message) => message,
                error => panic!("unexpected compiler output error: {error:?}"),
            };
            assert!(message.contains(&format!(
                "compiler {} exceeded the 128-byte limit",
                stream.name()
            )));
            let pid = wait_for_pid(&compiler_pid)
                .await
                .expect("oversized compiler did not report its process ID");
            assert_process_stops_after_abort(pid, "oversized compiler").await;
            assert_directory_becomes_empty(&cache_dir, "rejecting oversized compiler output").await;
        }
    }

    #[tokio::test]
    async fn native_runtime_preflight_abort_stops_the_compiler_process() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("blocking-compiler");
        let compiler_pid = dir.path().join("compiler.pid");
        let entrypoint = dir.path().join("workflow.ts");
        let cache_dir = dir.path().join("cache");

        write_blocking_compiler(&compiler, &compiler_pid);
        fs::write(&entrypoint, "export async function main() {}\n").unwrap();

        let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        )));
        let task_runtime = Arc::clone(&runtime);
        let task =
            tokio::spawn(async move { task_runtime.preflight(&native_spec("workflow.ts")).await });

        let Some(pid) = wait_for_pid(&compiler_pid).await else {
            task.abort();
            let _ = task.await;
            panic!("blocking compiler did not report its process ID");
        };
        assert!(process_is_running(pid));
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());

        assert_process_stops_after_abort(pid, "compiler").await;
        assert_directory_becomes_empty(&cache_dir, "aborting a Native TypeScript preflight").await;
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
    async fn native_runtime_cache_is_isolated_by_compiler_identity() {
        let dir = tempfile::tempdir().unwrap();
        let first_compiler = dir.path().join("first-compiler");
        let second_compiler = dir.path().join("second-compiler");
        let first_compile_log = dir.path().join("first-compile.log");
        let second_compile_log = dir.path().join("second-compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let request_log = dir.path().join("requests.jsonl");
        let cache_dir = dir.path().join("cache");
        let spec = native_spec("workflow.ts");

        write_rewriting_compiler(&first_compiler, &first_compile_log, "compiler-a");
        write_rewriting_compiler(&second_compiler, &second_compile_log, "compiler-b");
        write_runtime_source(
            &entrypoint,
            &request_log,
            "native-cache-compiler-marker",
            "a3s.flow.native_ts.v1",
        );

        let first_runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &first_compiler,
            &cache_dir,
            dir.path(),
        )));
        let first_preflight = first_runtime.preflight(&spec).await.unwrap();
        assert!(!first_preflight.cache_hit);
        let first_engine = FlowEngine::in_memory(first_runtime);
        let first_run_id = first_engine.start(spec.clone(), json!({})).await.unwrap();
        assert_eq!(
            first_engine
                .snapshot(&first_run_id)
                .await
                .unwrap()
                .output
                .unwrap()["marker"],
            "compiler-a"
        );

        let second_runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &second_compiler,
            &cache_dir,
            dir.path(),
        )));
        let second_preflight = second_runtime.preflight(&spec).await.unwrap();
        assert!(
            !second_preflight.cache_hit,
            "a different configured compiler must not reuse another compiler's artifact"
        );
        assert_eq!(second_preflight.source_hash, first_preflight.source_hash);
        assert_ne!(second_preflight.artifact, first_preflight.artifact);

        let second_engine = FlowEngine::in_memory(second_runtime);
        let second_run_id = second_engine.start(spec, json!({})).await.unwrap();
        assert_eq!(
            second_engine
                .snapshot(&second_run_id)
                .await
                .unwrap()
                .output
                .unwrap()["marker"],
            "compiler-b"
        );
        assert_eq!(compile_count(&first_compile_log), 1);
        assert_eq!(compile_count(&second_compile_log), 1);
        assert_eq!(fs::read_dir(&cache_dir).unwrap().count(), 2);
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
    async fn native_runtime_invocation_abort_stops_the_artifact_process() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("fake-compiler");
        let compile_log = dir.path().join("compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let runtime_pid = dir.path().join("runtime.pid");
        let cache_dir = dir.path().join("cache");

        write_fake_compiler(&compiler, &compile_log);
        write_blocking_runtime_source(&entrypoint, &runtime_pid);

        let runtime = Arc::new(NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        )));
        let engine = FlowEngine::in_memory(runtime);
        let task =
            tokio::spawn(async move { engine.start(native_spec("workflow.ts"), json!({})).await });

        let Some(pid) = wait_for_pid(&runtime_pid).await else {
            task.abort();
            let _ = task.await;
            panic!("blocking runtime artifact did not report its process ID");
        };
        assert!(process_is_running(pid));
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());

        assert_process_stops_after_abort(pid, "runtime artifact").await;
        assert_eq!(compile_count(&compile_log), 1);
    }

    #[tokio::test]
    async fn native_runtime_invocation_bounds_runtime_output_streams() {
        for stream in [TestOutputStream::Stdout, TestOutputStream::Stderr] {
            let dir = tempfile::tempdir().unwrap();
            let compiler = dir.path().join("fake-compiler");
            let compile_log = dir.path().join("compile.log");
            let entrypoint = dir.path().join("workflow.ts");
            let runtime_pid = dir.path().join("runtime.pid");
            let cache_dir = dir.path().join("cache");

            write_fake_compiler(&compiler, &compile_log);
            write_oversized_output_runtime_source(&entrypoint, &runtime_pid, stream);

            let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::new(
                &compiler,
                &cache_dir,
                dir.path(),
            ))
            .with_output_limits(128, 128);
            let spec = native_spec("workflow.ts");
            runtime.preflight(&spec).await.unwrap();
            let runtime = Arc::new(runtime);
            let engine = FlowEngine::in_memory(runtime);
            let error = tokio::time::timeout(Duration::from_secs(5), engine.start(spec, json!({})))
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "runtime {} output limit must fail before the child finishes",
                        stream.name()
                    )
                })
                .unwrap_err();

            let message = match error {
                FlowError::Runtime(message) => message,
                error => panic!("unexpected runtime output error: {error:?}"),
            };
            assert!(message.contains(&format!(
                "runtime {} exceeded the 128-byte limit",
                stream.name()
            )));
            let pid = wait_for_pid(&runtime_pid)
                .await
                .expect("oversized runtime did not report its process ID");
            assert_process_stops_after_abort(pid, "oversized runtime artifact").await;
            assert_eq!(compile_count(&compile_log), 1);
        }
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

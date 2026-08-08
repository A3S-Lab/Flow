#[cfg(all(feature = "native-ts", unix))]
mod native_ts_timeouts {
    use a3s_flow::{FlowEngine, FlowError, NativeTsRuntime, NativeTsRuntimeConfig, WorkflowSpec};
    use serde_json::json;
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

    fn write_fake_compiler(path: &Path, compile_log: &Path) {
        let content = format!(
            r#"#!/bin/sh
set -eu
printf 'compile\n' >> {compile_log}
cp "$2" "$4"
chmod +x "$4"
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

    fn write_success_runtime_source(path: &Path) {
        write_executable(
            path,
            r#"#!/bin/sh
set -eu
cat >/dev/null
printf '{"protocol":"a3s.flow.native_ts.v1","kind":"workflow","ok":true,"output":{"type":"complete","output":{"marker":"within-timeout"}}}\n'
"#,
        );
    }

    fn compile_count(path: &Path) -> usize {
        fs::read_to_string(path).unwrap_or_default().lines().count()
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

    async fn assert_process_stops(pid: u32, process_kind: &str) {
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
        assert!(stopped, "timing out must stop the {process_kind}");
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
        assert!(empty, "timing out must remove temporary cache artifacts");
    }

    #[tokio::test]
    async fn native_runtime_preflight_timeout_stops_the_compiler_process() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("blocking-compiler");
        let compiler_pid = dir.path().join("compiler.pid");
        let entrypoint = dir.path().join("workflow.ts");
        let cache_dir = dir.path().join("cache");

        write_blocking_compiler(&compiler, &compiler_pid);
        fs::write(&entrypoint, "export async function main() {}\n").unwrap();

        let runtime = Arc::new(
            NativeTsRuntime::new(NativeTsRuntimeConfig::new(
                &compiler,
                &cache_dir,
                dir.path(),
            ))
            .with_compile_timeout(Duration::from_secs(5)),
        );
        let task_runtime = Arc::clone(&runtime);
        let task =
            tokio::spawn(async move { task_runtime.preflight(&native_spec("workflow.ts")).await });

        let pid = wait_for_pid(&compiler_pid)
            .await
            .expect("timed compiler did not report its process ID");
        assert!(process_is_running(pid));
        let error = tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("compiler timeout did not complete")
            .unwrap()
            .unwrap_err();

        assert!(
            matches!(error, FlowError::Runtime(message) if message == "native TypeScript compiler timed out after 5s")
        );
        assert_process_stops(pid, "compiler").await;
        assert_directory_becomes_empty(&cache_dir).await;
    }

    #[tokio::test]
    async fn native_runtime_invocation_timeout_covers_blocked_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("fake-compiler");
        let compile_log = dir.path().join("compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let runtime_pid = dir.path().join("runtime.pid");
        let cache_dir = dir.path().join("cache");

        write_fake_compiler(&compiler, &compile_log);
        write_blocking_runtime_source(&entrypoint, &runtime_pid);

        let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        ))
        .with_invocation_timeout(Duration::from_secs(5));
        let spec = native_spec("workflow.ts");
        runtime.preflight(&spec).await.unwrap();
        let engine = FlowEngine::in_memory(Arc::new(runtime));
        let task = tokio::spawn(async move {
            engine
                .start(spec, json!({ "payload": "x".repeat(1024 * 1024) }))
                .await
        });

        let pid = wait_for_pid(&runtime_pid)
            .await
            .expect("timed runtime did not report its process ID");
        assert!(process_is_running(pid));
        let error = tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("runtime invocation timeout did not complete")
            .unwrap()
            .unwrap_err();

        assert!(
            matches!(error, FlowError::Runtime(message) if message == "native TypeScript runtime timed out after 5s")
        );
        assert_process_stops(pid, "runtime artifact").await;
        assert_eq!(compile_count(&compile_log), 1);
    }

    #[tokio::test]
    async fn native_runtime_timeouts_allow_fast_compile_and_invocation() {
        let dir = tempfile::tempdir().unwrap();
        let compiler = dir.path().join("fake-compiler");
        let compile_log = dir.path().join("compile.log");
        let entrypoint = dir.path().join("workflow.ts");
        let cache_dir = dir.path().join("cache");

        write_fake_compiler(&compiler, &compile_log);
        write_success_runtime_source(&entrypoint);

        let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::new(
            &compiler,
            &cache_dir,
            dir.path(),
        ))
        .with_compile_timeout(Duration::from_secs(5))
        .with_invocation_timeout(Duration::from_secs(5));
        let engine = FlowEngine::in_memory(Arc::new(runtime));
        let run_id = engine
            .start(native_spec("workflow.ts"), json!({}))
            .await
            .unwrap();
        let snapshot = engine.snapshot(&run_id).await.unwrap();

        assert_eq!(snapshot.output.unwrap()["marker"], "within-timeout");
        assert_eq!(compile_count(&compile_log), 1);
    }
}

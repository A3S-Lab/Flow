#[cfg(feature = "native-ts")]
use super::{read_bounded_output, NativeProcessOutputError};
use super::{NativeTsDependencyMode, NativeTsRuntime, NativeTsRuntimeConfig};
use crate::model::WorkflowSpec;
use std::path::Path;
use std::time::Duration;

#[test]
fn native_ts_default_cache_stays_under_a3s_state_root() {
    let config = NativeTsRuntimeConfig::default();

    assert_eq!(config.cache_dir, Path::new(".a3s/flow/native-ts"));
}

#[test]
fn native_ts_runtime_output_limits_are_configurable() {
    let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::default());

    assert_eq!(
        runtime.max_stdout_bytes(),
        NativeTsRuntime::DEFAULT_MAX_STDOUT_BYTES
    );
    assert_eq!(
        runtime.max_stderr_bytes(),
        NativeTsRuntime::DEFAULT_MAX_STDERR_BYTES
    );

    let runtime = runtime.with_output_limits(123, 45);
    assert_eq!(runtime.max_stdout_bytes(), 123);
    assert_eq!(runtime.max_stderr_bytes(), 45);
}

#[test]
fn native_ts_runtime_timeouts_are_opt_in_and_configurable() {
    let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::default());

    assert_eq!(runtime.compile_timeout(), None);
    assert_eq!(runtime.invocation_timeout(), None);

    let runtime = runtime
        .with_compile_timeout(Duration::from_secs(30))
        .with_invocation_timeout(Duration::from_secs(5));
    assert_eq!(runtime.compile_timeout(), Some(Duration::from_secs(30)));
    assert_eq!(runtime.invocation_timeout(), Some(Duration::from_secs(5)));
}

#[test]
fn native_ts_preflight_future_stays_bounded_for_small_stack_hosts() {
    let runtime = NativeTsRuntime::new(NativeTsRuntimeConfig::default())
        .with_dependency_mode(NativeTsDependencyMode::CompilerManifest);
    let spec = WorkflowSpec::native_ts("future-size", "1", "main.ts", "main");
    let future = runtime.preflight(&spec);
    let size = std::mem::size_of_val(&future);

    assert!(
        size <= 32 * 1024,
        "NativeTsRuntime::preflight future is {size} bytes"
    );
}

#[cfg(feature = "native-ts")]
#[tokio::test]
async fn native_ts_output_reader_accepts_exact_limit_and_rejects_next_byte() {
    let exact = read_bounded_output(&b"1234"[..], "stdout", 4)
        .await
        .unwrap();
    assert_eq!(exact, b"1234");

    let error = read_bounded_output(&b"12345"[..], "stdout", 4)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        NativeProcessOutputError::LimitExceeded {
            stream: "stdout",
            limit: 4
        }
    ));
}

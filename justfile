# A3S Flow

default:
    @just --list

# Format the crate
fmt:
    cargo fmt --all

# Check crate formatting
fmt-check:
    cargo fmt --all --check

# Check the current diff for whitespace errors
diff-check:
    git diff --check

# Run tests
test:
    cargo test --all-targets

# Type-check the crate
check:
    cargo check --all-targets

# Run the non-Postgres deep validation suite
deep-test-non-pg: fmt-check diff-check clippy-non-pg test-non-pg doc-non-pg examples-non-pg package-dry-run

# Run the real Postgres store and worker integration suite
postgres-test:
    test -n "${A3S_FLOW_POSTGRES_URL:-}" || (echo "A3S_FLOW_POSTGRES_URL is required" >&2; exit 1)
    cargo test --all-targets --features postgres -- --test-threads=1

# Run the opt-in real Bun compiler and workflow smoke test
native-ts-bun-test:
    test -n "${A3S_FLOW_NATIVE_TS_COMPILER:-}" || (echo "A3S_FLOW_NATIVE_TS_COMPILER is required" >&2; exit 1)
    cargo test --test native_ts_bun_smoke --features native-ts -- --ignored --exact native_ts_compiler_executes_real_bun_workflow

# Run strict linting across non-Postgres feature combinations
clippy-non-pg:
    cargo clippy --all-targets -- -D warnings
    cargo clippy --all-targets --no-default-features -- -D warnings
    cargo clippy --all-targets --no-default-features --features sqlite,native-ts -- -D warnings
    cargo clippy --all-targets --no-default-features --features boot,sqlite,native-ts -- -D warnings

# Run tests across non-Postgres feature combinations
test-non-pg:
    cargo test --all-targets
    cargo test --all-targets --no-default-features
    cargo test --all-targets --no-default-features --features boot
    cargo test --all-targets --no-default-features --features sqlite
    cargo test --all-targets --no-default-features --features native-ts
    cargo test --all-targets --no-default-features --features sqlite,native-ts
    cargo test --all-targets --no-default-features --features boot,sqlite,native-ts

# Build docs with warnings denied for non-Postgres feature combinations
doc-non-pg:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --no-default-features --features sqlite,native-ts
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --no-default-features --features boot,sqlite,native-ts

# Run executable examples that do not require Postgres
examples-non-pg:
    cargo run --quiet --no-default-features --example sequential_steps
    cargo run --quiet --no-default-features --example local_retention
    cargo run --quiet --no-default-features --example task_queue_durability
    cargo run --quiet --no-default-features --features boot --example boot_task_policy
    cargo run --quiet --no-default-features --features sqlite --example sqlite_durability
    cargo run --quiet --no-default-features --features sqlite --example sqlite_retention
    cargo run --quiet --no-default-features --features sqlite --example sqlite_worker
    cargo run --quiet --no-default-features --features native-ts --example native_ts_greeting
    cargo run --quiet --no-default-features --features native-ts --example native_ts_preflight

# Verify the crate package and publish payload without uploading it
package-dry-run:
    cargo package --locked --allow-dirty
    cargo publish --locked --dry-run --allow-dirty

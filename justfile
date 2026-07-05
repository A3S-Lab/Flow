# A3S Flow

default:
    @just --list

# Format the crate
fmt:
    cargo fmt --all

# Run tests
test:
    cargo test --all-targets

# Type-check the crate
check:
    cargo check --all-targets

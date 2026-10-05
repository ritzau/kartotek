# List the available development commands.
default:
    @just --list

# Build the development executable.
build:
    cargo build --locked

# Build the optimized executable.
release:
    cargo build --locked --release

# Run all Rust tests.
test:
    cargo test --locked

# Format Rust and this justfile.
fmt:
    cargo fmt --all
    just --unstable --fmt

# Check formatting without modifying files.
fmt-check:
    cargo fmt --all -- --check
    just --unstable --fmt --check

# Analyze all Rust targets, treating warnings as errors.
lint:
    cargo clippy --locked --all-targets -- -D warnings

# Check whitespace without modifying files.
whitespace-check:
    prek --config tools/whitespace-check.toml run --all-files

# Check whitespace, formatting, static analysis, tests, and hook configuration.
check: whitespace-check fmt-check lint test
    prek validate-config prek.toml
    prek validate-config tools/whitespace-check.toml

# Run all hooks, including automatic whitespace cleanup.
hooks:
    prek run --all-files

# Scan a directory without modifying its files.
[positional-arguments]
scan directory:
    cargo run --locked -- scan "$1"

# Run Kartotek with the given command-line arguments.
[positional-arguments]
run +args:
    cargo run --locked -- "$@"

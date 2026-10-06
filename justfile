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

# Save a scan in a catalog outside the source directory.
[positional-arguments]
scan directory catalog:
    cargo run --locked -- --catalog "$2" scan "$1"

# List saved scans, including incomplete and interrupted scans.
[positional-arguments]
scans catalog:
    cargo run --locked -- --catalog "$1" scans

# Save metadata and staged SHA-256 hashes in a catalog.
[positional-arguments]
hash-scan directory catalog:
    cargo run --locked --release -- --catalog "$2" scan "$1" --hash

# List matching full-file hashes from a saved scan without accessing the source.
[positional-arguments]
duplicates catalog scan:
    cargo run --locked -- --catalog "$1" duplicates "$2"

# Run Kartotek with the given command-line arguments.
[positional-arguments]
run +args:
    cargo run --locked -- "$@"

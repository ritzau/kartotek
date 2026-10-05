# Kartotek

Kartotek inventories disks and backups to help you find files, identify duplicate content, and understand what is stored where, even when a disk is disconnected.

Status: an initial read-only directory scanner is implemented. Persistent catalogs,
offline search, resume, and duplicate detection are planned.

## Usage

Enable the workspace environment as described below; the first Rust command
automatically installs Rust 1.99.0.

```sh
just scan /path/to/disk
```

The scanner recursively prints regular files as a byte size, a tab, and a quoted
path. Paths use Rust debug escaping so embedded tabs and newlines do not split
records. Output order is unspecified. Symlinks (including a symlink scan root)
are not followed; special files are skipped. Files are not opened or hashed.

Read failures are reported on stderr and scanning continues when possible. Exit
status is 0 for a complete traversal, 1 for an incomplete or failed scan, and 2
for invalid command syntax. Interrupting the process leaves partial output with
no completion message; scans are not saved or resumable yet. A complete traversal
is an observation, not a filesystem snapshot. Concurrent source changes can affect
results; path checks cannot eliminate races with directory replacement.

## Development

The Rust executable has no third-party dependencies. Development hooks use
[prek](https://prek.j178.dev/), a Rust Git hook runner with built-in whitespace
fixers. See [DEVELOPMENT.md](DEVELOPMENT.md) for dependency details.

From Bash or zsh, source the setup script in each new shell:

```sh
source tools/setup-env
just check
```

The script can also be sourced using an absolute path from another directory.
It needs Git, Bash, curl or wget, OpenSSL, and standard Unix archive tools.
A system C linker/build toolchain is required when compiling Rust. Supported platforms
are Linux with glibc and macOS, on x86_64 or ARM64. No separate
Rust, just, prek, Python, or uv installation is required. The environment remains
active until the shell exits; it does not automatically unload when you change
directories.

The setup installs pinned rustup, prek, and just into `.cache/bin`
and adds that directory to PATH while you work here. Rustup downloads the version
and components in `rust-toolchain.toml` on the first Rust command, including
rustfmt and Clippy. Entering the workspace or listing recipes with `just`
does not download the Rust toolchain. Builds, tests, formatting, linting, and
Rust hooks trigger installation when needed.
`CARGO_HOME` is set to `.cache` and `RUSTUP_HOME` to `.cache/rustup` in this
workspace, keeping Rust downloads and configuration local. The first load
requires network access for the small tools; the first Rust command also needs
network access for the toolchain. Later loads and builds reuse installed tools
offline. Downloads are ignored by Git. No global prek,
just, or Rust installation is needed. A system C linker is still required for
building; install your operating system's build tools if it is missing.

Setup installs the Git hook and preserve unrelated existing hooks
through prek's migration mode. Rust and justfile hooks explicitly activate the
workspace paths, so IDE commits do not depend on an activated shell. Prek reads `prek.toml` and runs the checks before each
commit. It removes trailing whitespace, ensures a single final newline in
nonempty text files, formats Rust with rustfmt, and runs Clippy with warnings
as errors. No workspace Python environment or activation is needed. If a hook
changes files, review and stage the changes before committing again.
`.editorconfig` applies the same whitespace conventions in supporting editors.
Rust tools use the pinned toolchain.

Use `just` for routine development; running it without arguments lists recipes:

```sh
just
just build
just release
just test
just fmt
just lint
just check
just hooks
just scan /path/to/disk
just run --help
```

`just check` checks whitespace, Rust and justfile formatting, runs Clippy and
tests, and validates hook configuration without modifying source files. `just hooks` runs all
hooks, including whitespace cleanup. `just fmt-check` checks formatting only.
All Cargo recipes use locked dependencies; Rust commands use the pinned toolchain.
Ambient `RUSTUP_TOOLCHAIN` overrides are cleared when activating this workspace;
explicit `cargo +<toolchain>` commands remain available for deliberate experiments.

Release download URLs and
SHA-256 hashes are recorded in `tools/tool-releases.txt`; downloaded binaries
and archives are verified before installation. Setup does not execute downloaded
shell scripts.

## Initial scope

- Local disk inventory stored in SQLite.
- Search stored inventories while disks are offline.
- Explicit scan states, including interruptions and unreadable files.
- Resume interrupted inventories.
- Duplicate candidates by size, followed by content hashing.
- CLI first, interactive terminal UI next; a web UI may follow.

An inventory records observations, not the current truth of a disk. Incomplete scans must never appear complete. Source files are read-only; automatic deletion is outside the initial scope.

## Technical direction

Rust, one executable, synchronous code initially. Separate inventory, catalog storage, duplicate analysis, and presentation into responsibility-named modules. Add concurrency or separate crates only for a concrete need.

Pin the Rust toolchain, commit Cargo.lock, and build with --locked once code is introduced. System linker and libraries are permitted; a pinned sysroot and fully hermetic environment are not required.

Keep dependencies few and justify the complete transitive dependency tree. SQLite is the planned catalog, with no separate database service.

## First milestone

Inventory a disk, interrupt and resume the scan, disconnect the disk, and search its stored catalog.

## License

Licensed under the BSD Zero Clause License (0BSD). See [LICENSE](LICENSE).

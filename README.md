# Kartotek

Kartotek inventories disks and backups to help you find files, identify duplicate content, and understand what is stored where, even when a disk is disconnected.

Status: an initial read-only directory scanner is implemented. Persistent catalogs,
offline search, resume, and duplicate detection are planned.

## Usage

Install Rust through rustup; the repository pins Rust 1.99.0.

```sh
cargo run --locked -- scan /path/to/disk
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

The Rust executable has no third-party dependencies. Install Rust through rustup,
[prek](https://github.com/j178/prek/releases/tag/v0.5.4) 0.5.4, and
[just](https://github.com/casey/just/releases/tag/1.58.0) 1.58.0, with their commands
on PATH. See [DEVELOPMENT.md](DEVELOPMENT.md) for tooling dependency details.

```sh
prek install
just check
```

The hooks remove trailing whitespace, ensure a single final newline in nonempty
text files, format Rust and the justfile, and run Clippy with warnings as errors.
If a hook changes files, review and stage the changes before committing again.
Hooks do not stage fixes. `.editorconfig` supplies the same whitespace conventions
to supporting editors. Prek uses built-in fixers without Python or uv.

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

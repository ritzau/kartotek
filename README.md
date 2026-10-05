# Kartotek

Kartotek inventories disks and backups to help you find files, identify duplicate content, and understand what is stored where, even when a disk is disconnected.

Status: read-only directory scans are saved in SQLite catalogs and can be listed
while source disks are offline. Optional staged SHA-256 hashing finds duplicate
content within a scan. File search and resume are planned.

## Usage

Enable the workspace environment as described below; the first Rust command
automatically installs Rust 1.99.0.

```sh
just scan /path/to/disk /path/to/catalog.sqlite
just scans /path/to/catalog.sqlite
just hash-scan /path/to/disk /path/to/catalog.sqlite
just duplicates /path/to/catalog.sqlite 2
```

Choose a catalog outside the scanned directory, with an existing parent directory.
Kartotek rejects catalogs inside the source tree, including aliases through parent
symlinks, and rejects linked catalog or SQLite sidecar files. The catalog is the
only writable data; source files remain read-only. Path checks cannot eliminate
concurrent replacement races or aliases through mount points.

Without `just`, use `kartotek --catalog <file> scan <directory>` or
`kartotek --catalog <file> scans`. Listing requires an existing catalog and does
not access the source disk.

The scanner recursively prints regular files as a byte size, a tab, and a quoted
absolute path. Paths use Rust debug escaping so embedded tabs and newlines do not
split records. Output order is unspecified. Symlinks (including a symlink scan root)
are not followed; special files are skipped. The default scan collects metadata
without opening file contents. Observations include size, device/inode identity,
link count, modification/change timestamps, mode, and owner/group IDs.

`hash-scan` (or `scan <directory> --hash`) explicitly enables content reads. After
metadata collection, Kartotek saves SHA-256 hashes of the first 64 KiB of each file.
For files at or below 64 KiB this is also the full hash. Larger files are hashed
in full only when another distinct device/inode pair has the same size and prefix
hash. Hard-link aliases alone do not trigger full reads or duplicate groups.
Reads are bounded by the observed size; metadata and identity are checked before
and after reading, and the full pass checks the saved prefix again. Changes and
read failures are saved and make the scan incomplete. These checks cannot provide
a filesystem snapshot or detect every concurrent modification.

`duplicates <scan-id>` reads saved full hashes while the source is offline. It
prints groups with matching sizes and full SHA-256 digests, including paths and
device/inode pairs. Results from incomplete or interrupted scans are explicitly
partial. Unique prefixes remain candidates with no full hash; a prefix match
alone is never reported as duplicate content. No source files are modified.

Read failures are reported on stderr and scanning continues when possible. Exit
status is 0 for a complete traversal, 1 for an incomplete or failed scan, and 2
for invalid command syntax. Each file observation and read failure is committed
before it is reported. Saved scans have explicit running, complete, incomplete,
or interrupted states. A stopped process leaves its scan running until the next
catalog open acquires exclusive ownership and recovers it as interrupted. Already
committed observations are retained, and the interruption time remains unknown.
Handled failures, such as a closed output pipe, record an interrupted state and
the error immediately when storage is still available. Scans are not resumable yet.
A complete traversal is an observation, not a filesystem snapshot. Concurrent source changes can affect
results; path checks cannot eliminate races with directory replacement.

`scans` prints tab-separated ID, state, file count, failure count, start and finish
times (Unix seconds, or `unknown`), quoted source root, a quoted note, hashing
enabled, and successful prefix/full hash counts. It may
update abandoned scan states during recovery. Only one Kartotek process can use a
catalog at a time; concurrent commands fail with a database-lock error rather
than retrying. Catalog schema versions are checked before use; unrelated SQLite
databases and unsupported versions are rejected. Schema version 1 catalogs are
transactionally upgraded to version 2, preserving previous observations and
leaving their newly added metadata and hashes unknown.

## Development

SQLite storage uses [rusqlite](https://github.com/rusqlite/rusqlite) with bundled
SQLite; the system C compiler is needed for builds, with no separate SQLite
development package required. See [DEPENDENCIES.md](DEPENDENCIES.md) for the full
runtime/build dependency graph and licenses. Development hooks use
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

Alternatively, install [direnv](https://direnv.net/) and enable its shell
integration once (for zsh, add `eval "$(direnv hook zsh)"` to `.zshrc`):

```sh
direnv allow
just check
```

Direnv loads the same `tools/setup-env` script and supplies its `fetchurl`
functionality for hash-verified downloads, so this path does not require OpenSSL.
It automatically activates and unloads the workspace environment when entering
and leaving the directory, and watches the setup scripts and `.cache/bin`.

The shared setup installs pinned rustup, prek, and just into `.cache/bin`
and adds that directory to PATH while you work here. Rustup downloads the version
and components in `rust-toolchain.toml` on the first Rust command, including
rustfmt and Clippy. Entering the workspace or listing recipes with `just`
does not download the Rust toolchain. Builds, tests, formatting, linting, and
Rust hooks trigger installation when needed.
`CARGO_HOME` is set to `.cache` and `RUSTUP_HOME` to `.cache/rustup` in this
workspace, keeping Rust downloads and configuration local. The first load
requires network access for the small tools; the first Rust command also needs
network access for the toolchain. Later loads and builds reuse installed tools
offline. Downloads are ignored by Git. With direnv, changes to `.cache/bin`
trigger a reload on the next prompt, restoring missing tools as needed. No global prek,
just, or Rust installation is needed. A system C linker is still required for
building; install your operating system's build tools if it is missing.

Both setup paths install the Git hook and preserve unrelated existing hooks
through prek's migration mode. Rust and justfile hooks explicitly activate the
workspace paths, so IDE commits do not depend on a direnv-aware shell. Prek reads `prek.toml` and runs the checks before each
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
just scan /path/to/disk /path/to/catalog.sqlite
just scans /path/to/catalog.sqlite
just run --help
```

`just check` checks whitespace, Rust and justfile formatting, runs Clippy and
tests, and validates hook configuration without modifying source files. `just hooks` runs all
hooks, including whitespace cleanup. `just fmt-check` checks formatting only.
All Cargo recipes use locked dependencies; Rust commands use the pinned toolchain.
Ambient `RUSTUP_TOOLCHAIN` overrides are cleared when activating this workspace;
explicit `cargo +<toolchain>` commands remain available for deliberate experiments.

GitHub Actions runs the same `source tools/setup-env` and `just check` commands
on Linux for pushes and pull requests. Release download URLs and
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

# Development tooling dependencies

The Kartotek executable has no third-party dependencies. Rustfmt and Clippy
ship with the pinned Rust toolchain. External tools are installed locally under
`.cache`; they add no crates to Kartotek's Cargo.lock or runtime.

| Tool | Purpose | Maintainer | License | Complete upstream build dependency graph |
| --- | --- | --- | --- | --- |
| [rustup](https://github.com/rust-lang/rustup) 1.29.1 | Install pinned Rust and components on first use | Rust project | MIT OR Apache-2.0 | [Cargo.lock](https://github.com/rust-lang/rustup/blob/1.29.1/Cargo.lock) |
| [prek](https://github.com/j178/prek) 0.5.4 | Git hooks and built-in whitespace checks | j178 and contributors | MIT | [Cargo.lock](https://github.com/j178/prek/blob/v0.5.4/Cargo.lock) |
| [just](https://github.com/casey/just) 1.58.0 | Routine development commands | Casey Rodarmor and contributors | CC0-1.0 | [Cargo.lock](https://github.com/casey/just/blob/1.58.0/Cargo.lock) |

`tools/tool-releases.txt` records immutable release URLs, versions, SHA-256 SRI
hashes, and archive members for Linux (glibc) and macOS on x86_64 and ARM64.
Rustup hashes come from the Rust project's release checksum files; prek and just
hashes come from their official GitHub release asset metadata. Every downloaded
binary or archive is verified before installation. No downloaded shell script
is executed. Archives are unpacked only for the named binary, then installed
with an atomic rename. Matching installed versions are reused offline.

`tools/setup-env` supports Bash and zsh, bootstraps tools in a subshell, and
activates the calling shell only after success. `tools/bootstrap` installs tools
and calls `prek install`, preserving unrelated hooks through migration mode.
The environment sets `CARGO_HOME` to `.cache`, `RUSTUP_HOME` to `.cache/rustup`,
and enables `RUSTUP_AUTO_INSTALL`. Ambient `RUSTUP_TOOLCHAIN` overrides are
cleared so the repository's `rust-toolchain.toml` determines the toolchain.
The compiler, Cargo, standard library, rustfmt, and Clippy download on the first
Rust tool command. Listing just recipes or activating the environment does not
install the Rust toolchain. System linker/build tools remain prerequisites.

[direnv](https://direnv.net/), maintained by the direnv project under MIT, is
optional. Its complete upstream build dependencies are recorded in
[go.mod](https://github.com/direnv/direnv/blob/master/go.mod) and
[go.sum](https://github.com/direnv/direnv/blob/master/go.sum).
`.envrc` loads the shared setup and watches its scripts, release manifest,
`.cache/bin`, and `rust-toolchain.toml`. This path uses `direnv fetchurl` for
verified caching. Without direnv, `tools/fetch-file` uses curl or wget and
OpenSSL to verify and cache releases in `.cache/downloads`. Cached files are
reverified before reuse; failed downloads and mismatched hashes are rejected.
OpenSSL is a system setup tool maintained by the OpenSSL project: version 3.x
uses Apache-2.0, older system versions use their respective upstream licenses.
It adds no Kartotek package dependencies.

`tools/activate-env` sets workspace paths without downloading or installing
hooks. `tools/run-in-env` uses it for Rust and justfile hooks, allowing commits
from IDEs and shells that have not loaded setup. Hooks run whitespace cleanup,
then the independent Rust/justfile formatters, then Clippy. `just check` uses
read-only whitespace hooks from `tools/whitespace-check.toml` as well as format,
lint, and test checks. Fixing hooks remain available with `just hooks`.

CI runs setup and `just check` on Linux, with a read-only GitHub token
and no persisted checkout credentials. It uses GitHub-maintained
[actions/checkout](https://github.com/actions/checkout) v7.0.1 under MIT, pinned
to commit `3d3c42e5aac5ba805825da76410c181273ba90b1`. Its bundled Node dependencies
are recorded in the [versioned package-lock.json](https://github.com/actions/checkout/blob/3d3c42e5aac5ba805825da76410c181273ba90b1/package-lock.json).
This dependency runs only in CI, not in Kartotek or local setup.

No dependencies are vendored or modified. When upgrading a tool, update every
supported platform's version, URL, hash, and archive member in the release
manifest. Update the minimum prek version in both hook configurations when
needed. Review upstream dependency and license changes, then verify fresh
setup, cached reuse, integrity failures, `just check`, and `just hooks`.

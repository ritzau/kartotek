# Runtime and build dependencies

Persistent catalogs need a transactional store that preserves committed observations
and distinguishes unfinished scans. [rusqlite](https://github.com/rusqlite/rusqlite)
0.40.2 supplies a maintained, MIT-licensed Rust interface to SQLite. Its default
features are disabled; only `bundled` is enabled. This avoids the optional statement
cache and WebAssembly dependency graphs. No async runtime or framework is added.

Bundled SQLite keeps builds independent of system SQLite development packages and
pins the database engine through Cargo.lock. A system C compiler is still required.
The complete resolved graph is committed in [Cargo.lock](Cargo.lock); reproduce it
with `cargo tree --locked` using the workspace toolchain:

```text
kartotek
├── libc 0.2.190
├── sha2 0.10.9
│   ├── cfg-if 1.0.5
│   ├── cpufeatures 0.2.17
│   └── digest 0.10.7
│       ├── block-buffer 0.10.4
│       │   └── generic-array 0.14.7
│       │       ├── typenum 1.20.1
│       │       [build dependencies]
│       │       └── version_check 0.9.5
│       └── crypto-common 0.1.7
│           ├── generic-array 0.14.7 (shared)
│           └── typenum 1.20.1 (shared)
└── rusqlite 0.40.2
    ├── bitflags 2.13.2
    ├── fallible-iterator 0.3.0
    ├── fallible-streaming-iterator 0.1.9
    ├── libsqlite3-sys 0.38.2
    │   [build dependencies]
    │   ├── cc 1.6.0
    │   │   ├── find-msvc-tools 0.1.14
    │   │   └── shlex 2.0.1
    │   ├── pkg-config 0.3.34
    │   └── vcpkg 0.2.15
    └── smallvec 1.16.2
```

| Package | Purpose | Upstream maintainers | License |
| --- | --- | --- | --- |
| rusqlite | Safe SQLite access | rusqlite project | MIT |
| libsqlite3-sys | SQLite C bindings and bundled build | rusqlite project | MIT |
| bitflags | SQLite option flags | bitflags project | MIT OR Apache-2.0 |
| fallible-iterator | Fallible query iteration | Steven Fackler | MIT OR Apache-2.0 |
| fallible-streaming-iterator | Fallible borrowed-row iteration | Steven Fackler | MIT OR Apache-2.0 |
| smallvec | Small inline buffers in rusqlite | Servo/rust-smallvec project | MIT OR Apache-2.0 |
| cc | Compile bundled SQLite | rust-lang/cc-rs project | MIT OR Apache-2.0 |
| find-msvc-tools | Compiler discovery in cc | rust-lang/cc-rs project | MIT OR Apache-2.0 |
| shlex | Parse compiler argument strings | rust-shlex project | MIT OR Apache-2.0 |
| pkg-config | System-library discovery in libsqlite3-sys build support | rust-lang/pkg-config-rs project | MIT OR Apache-2.0 |
| vcpkg | Windows-library discovery in libsqlite3-sys build support | vcpkg-rs project | MIT OR Apache-2.0 |

The build dependencies are part of the resolved graph, including discovery code
that the bundled build does not use to link a system SQLite library. Package
versions, checksums, licenses, and origins were checked against the downloaded
crate metadata. No additional test dependencies are used.

The libsqlite3-sys crate contains the SQLite 3.53.2 amalgamation, originating from
[SQLite](https://sqlite.org/) and maintained by its core project. SQLite's
[deliverable code is public domain](https://sqlite.org/copyright.html). Its source
identifier in the pinned crate is:

```text
2026-06-03 19:12:13 d6e03d8c777cfa2d35e3b60d8ec3e0187f3e9f99d8e2ee9cac695fd6fcdf1a24
```

No upstream sources are copied into this repository or locally patched. The
SQLite sources and the crate build configuration are supplied unchanged through
the checksum-pinned libsqlite3-sys package. Review licenses, bundled SQLite
provenance, and the entire resolved graph when updating dependencies.


Staged content hashing adds [sha2](https://github.com/RustCrypto/hashes) 0.10.9,
maintained by RustCrypto and licensed MIT OR Apache-2.0. Defaults are disabled;
only `std` is enabled. SHA-256 provides a standard, collision-resistant digest
without implementing cryptography locally. [libc](https://github.com/rust-lang/libc)
0.2.190 supplies platform constants for safe `OpenOptions` calls that reject
final-component symlinks and avoid blocking on a replaced FIFO. Own code contains
no unsafe blocks. The full addition is ten crates, listed below; cpufeatures also
uses libc on some target platforms. No additional test dependencies are added.

| Added package | Purpose | Upstream maintainers | License |
| --- | --- | --- | --- |
| sha2 0.10.9 | SHA-256 implementation | RustCrypto/hashes | MIT OR Apache-2.0 |
| libc 0.2.190 | Native open flags | rust-lang/libc | MIT OR Apache-2.0 |
| cfg-if 1.0.5 | Conditional implementation selection | rust-lang/cfg-if | MIT OR Apache-2.0 |
| cpufeatures 0.2.17 | CPU hash acceleration detection | RustCrypto/utils | MIT OR Apache-2.0 |
| digest 0.10.7 | Digest interfaces | RustCrypto/traits | MIT OR Apache-2.0 |
| block-buffer 0.10.4 | Hash block buffering | RustCrypto/utils | MIT OR Apache-2.0 |
| crypto-common 0.1.7 | Shared cryptographic interfaces | RustCrypto/traits | MIT OR Apache-2.0 |
| generic-array 0.14.7 | Fixed-size digest/block arrays | fizyk20/generic-array | MIT |
| typenum 1.20.1 | Compile-time array lengths | paholg/typenum | MIT OR Apache-2.0 |
| version_check 0.9.5 | Build-time compiler capability checks | Sergio Benitez | MIT OR Apache-2.0 |

# Catalog format

Schema version 2 uses SQLite application ID `0x4b415254`. Its tables and constraints
are in [src/catalog_schema.sql](src/catalog_schema.sql). Source roots and paths are
Unix byte strings stored as BLOBs, preserving non-UTF-8 names on the supported
Linux/macOS platforms. File sizes are decimal TEXT to preserve the full `u64`
range. Device, inode, and link count use the same exact unsigned representation.
Native modification/change timestamps include nanoseconds; mode and ownership
are retained. SHA-256 digests are 32-byte BLOBs. Hash states distinguish unrequested,
pending, prefix-only, full, and failed observations. Version 1 upgrades through
[src/catalog_migrate_v2.sql](src/catalog_migrate_v2.sql) in a transaction; historical
metadata/hash fields remain NULL rather than fabricated. Read failures retain a path, I/O error kind, and message. Times are Unix
seconds supplied by the command layer; recovery does not invent a stop time.

Each observation is committed independently with FULL synchronization. Exclusive
connection ownership and DELETE journaling retain the database lock across
commits, so recovery cannot relabel a live scan. This favors a simple durable
prefix over batching speed; concurrent commands fail immediately. Storage or
output failures stop the scan, and failure to finalize leaves a running record
for recovery rather than claiming completion. Resume is not implemented yet. A complete hashing scan requires no unresolved
hashes, no read failures, and no prefix-only files still needing a full hash.

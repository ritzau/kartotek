# Kartotek

Kartotek inventories disks and backups to help you find files, identify duplicate content, and understand what is stored where, even when a disk is disconnected.

Status: planning. No implementation yet.

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

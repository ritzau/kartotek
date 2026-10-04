# Agent instructions

## Purpose and priorities

Build Kartotek: a local catalog of disks and backups, searchable offline, with duplicate detection. Priorities, used as tie-breakers: user integrity, system integrity, reproducibility, user experience, functionality.

Ship small useful increments. Keep the setup simple and avoid speculative abstraction.

## Architecture

- Use idiomatic Rust, composition, and traits where an interface is useful. No implementation inheritance or DI containers.
- Pass dependencies explicitly. Do not carry a generic context object through the code.
- Every module and type should have a one-sentence responsibility. No util/common/helpers/misc grab bags.
- Prefer pure functions. State belongs in the smallest owning unit; invariant-bearing types have private fields.
- No mutable globals. Supply clocks and randomness explicitly where needed.
- Prefer single-thread confinement, then an internal mutex, then atomics only with clear justification. Keep locks private and never invoke caller-supplied code under a lock.
- Do not block a bounded pool thread on work that can re-enter the pool.
- Represent complex state and legal transitions explicitly. Document concurrency contracts.
- Start with one executable and synchronous code. Separate inventory, storage, duplicate analysis, and UI without premature crate splitting.
- Ownership must be transferred, not shared, across a C ABI or language boundary. Preserve native failure semantics at runtime boundaries.

## Integrity and failures

- Treat source disks and files as read-only. Do not add deletion or other source mutations without explicit authorization.
- Inventories are observations. Preserve scan completeness, read failures, interruptions, and uncertainty; never label partial results complete.
- Files can change or disappear between validation and use. Handle these as recoverable failures.
- Recoverable failures are values, not panics. Broken invariants terminate the process; fatal logging and backtraces are best effort.
- Validate external input at each trust boundary. Interior assertions guard invariants and do not replace input validation.
- The client owns retries; lower layers propagate failures.

## Dependencies and setup

- Pin the Rust version, commit Cargo.lock, and use --locked for builds and checks once the Rust project exists.
- System linker and libraries are permitted. A pinned sysroot and fully hermetic build are not required for this project.
- Every new dependency must be justified: purpose, maintainership, license, and complete transitive dependency impact. Record this in the PR or commit description.
- Do not introduce an async runtime, framework, or utility collection without a concrete need.
- Vendored code records origin, version, license, and local changes.

## Interfaces and configuration

- Own-code APIs may change when all clients are updated together. Externally consumed breaking changes must be explicit and versioned.
- Configuration comes from one system: file, then environment, then command line. Defaults live in one place.
- Secrets never enter the repository, distribution, argv, or environment; deliver through stdin or a file descriptor from a vault.
- Substantial or risky features should default off behind runtime configuration; retire flags after rollout.

## Verification

- Verify behavior proportionately to the change. Test interruptions, failure states, and integrity boundaries when implementing them.
- Once code exists, run formatting, linting, and relevant tests with the pinned toolchain and locked dependencies. Report checks actually run and any limitations.
- Keep README aligned with implemented behavior; distinguish plans from working features.
- No force-pushes, history rewrites, or destructive repository changes without explicit authorization.

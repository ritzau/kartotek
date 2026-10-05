# Development tooling dependencies

The Kartotek executable has no third-party dependencies. Rustfmt and Clippy
ship with the pinned Rust toolchain. Prek and just are development tools;
they add no crates to Kartotek's Cargo.lock or runtime.

| Tool | Purpose | Maintainer | License | Complete upstream build dependency graph |
| --- | --- | --- | --- | --- |
| [prek](https://github.com/j178/prek) 0.5.4 | Git hooks and built-in whitespace checks | j178 and contributors | MIT | [Cargo.lock](https://github.com/j178/prek/blob/v0.5.4/Cargo.lock) |
| [just](https://github.com/casey/just) 1.58.0 | Routine development commands | Casey Rodarmor and contributors | CC0-1.0 | [Cargo.lock](https://github.com/casey/just/blob/1.58.0/Cargo.lock) |

Hooks run built-in whitespace cleanup, Rust and justfile formatting, then Clippy.
`just check` uses read-only whitespace checks plus formatting, lint, and tests.
`just hooks` runs fixing hooks; fixes remain unstaged for review.

No dependencies are vendored or modified. Review upstream dependency and license
changes when upgrading tools.

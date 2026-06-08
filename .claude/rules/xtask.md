---
paths:
  - "crates/xtask/**"
  - ".cargo/config.toml"
---

# Xtask — Project Automation

`cargo xtask` is the project's automation framework, implemented as a workspace crate with a cargo alias.

## How it works

- Subcommands live in `crates/xtask/src/` — each is a module with a `pub fn run() -> Result<()>`
- The `Command` enum in `main.rs` maps CLI subcommands to modules
- The cargo alias `xtask = "run --package xtask --"` in `.cargo/config.toml` enables `cargo xtask <cmd>`
- Some subcommands have shortcut aliases (e.g., `cargo dev-setup` → `cargo xtask dev-setup`)

## Available commands

| Command | Alias | Purpose |
|---------|-------|---------|
| `cargo xtask dev-setup` | `cargo dev-setup` | Install required dev tools (cargo-nextest, wasm-bindgen-cli) |
| `cargo xtask lint` | — | Run custom lint checks |
| `cargo xtask create-worktree` | `cargo create-worktree` | Create git worktree + CLAUDE.local.md context stub (braid needs no redirect) |
| `cargo xtask braid-snapshot` | — | Write backup-only `braid export` to `.braid/snapshot.jsonl` (one-directional; never re-import) |
| `cargo xtask verify` | — | Full project verification (build + tests for Rust and hub-client) |

## Dev tool version pinning

Dev tools whose versions must match Cargo.lock (e.g., `wasm-bindgen-cli`) are installed
via `cargo xtask dev-setup`, which reads the locked version automatically. Never hardcode
these versions in CI workflows or documentation — always use `cargo xtask dev-setup`.

## Adding a new subcommand

1. Create `crates/xtask/src/<name>.rs` with `pub fn run() -> Result<()>`
2. Add `mod <name>;` in `main.rs`
3. Add variant to `Command` enum with doc comment
4. Add match arm in `main()`
5. Optionally add a shortcut alias in `.cargo/config.toml`
6. Update the doc comment at the top of `main.rs`

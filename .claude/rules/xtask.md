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
| `cargo xtask dev-setup` | `cargo dev-setup` | Install required dev tools (cargo-nextest, wasm-pack) |
| `cargo xtask lint` | — | Run custom lint checks |
| `cargo xtask verify` | — | Full project verification (build + tests for Rust and hub-client) |

## Adding a new subcommand

1. Create `crates/xtask/src/<name>.rs` with `pub fn run() -> Result<()>`
2. Add `mod <name>;` in `main.rs`
3. Add variant to `Command` enum with doc comment
4. Add match arm in `main()`
5. Optionally add a shortcut alias in `.cargo/config.toml`
6. Update the doc comment at the top of `main.rs`

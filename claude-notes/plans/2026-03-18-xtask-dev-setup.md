# Plan: Add `cargo xtask dev-setup` subcommand

## Overview

Add a `dev-setup` subcommand to the existing xtask crate that installs development prerequisites. This gives contributors a single command after cloning:

```bash
cargo xtask dev-setup
```

The `rust-toolchain.toml` handles toolchain/components/targets automatically. `dev-setup` covers the remaining tools that Cargo can't auto-install.

## What it installs

| Tool | Install command | Purpose |
|------|----------------|---------|
| cargo-nextest | `cargo install cargo-nextest --locked` | Test runner (required) |
| wasm-pack | `cargo install wasm-pack --locked` | WASM builds for hub-client |

### cargo-binstall optimization

If `cargo-binstall` is available, use it for faster installs (pre-built binaries). Fall back to `cargo install` otherwise. Don't install binstall itself — that's a user choice.

## Windows awareness

- Detect Windows via `cfg!(target_os = "windows")` or `std::env::consts::OS`
- Print a note about v8 test limitations and the `--exclude` flags needed
- Optionally print the full exclude command for copy-paste

## Behavior

1. Check which tools are already installed (skip if present)
2. Install missing tools
3. Print platform-specific notes (Windows v8 excludes, pampa-fuzz skip)
4. Print a summary of what's ready

### Skip-if-present detection

```rust
// Check if a binary is on PATH
fn is_installed(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .output()
        .is_ok()
}
```

Check for `cargo-nextest` via `cargo nextest --version` and `wasm-pack` via `wasm-pack --version`.

## Work Items

- [x] Add `DevSetup` variant to the `Command` enum in `crates/xtask/src/main.rs`
- [x] Create `crates/xtask/src/dev_setup.rs` with install logic
- [x] Implement tool detection (already-installed check)
- [x] Implement install via cargo-binstall (if available) or cargo install
- [x] Add Windows platform notes output
- [x] Test on Windows: verify it installs nextest and wasm-pack correctly
- [x] Update CONTRIBUTING.md to reference `cargo xtask dev-setup`
- [x] Add brief xtask overview to CLAUDE.md (what it is, where subcommands live)

## Example output

```
$ cargo xtask dev-setup

Checking development tools...
  cargo-nextest: already installed (0.9.96)
  wasm-pack: installing... done (0.13.1)

Platform: Windows
  Note: 12 crates require --exclude flags for test compilation (v8 rlib limitation).
  See claude-notes/instructions/windows-dev.md or run:

    cargo nextest run --workspace \
      --exclude quarto-system-runtime \
      --exclude pampa \
      ...

All development tools ready.
```

## Non-goals

- Don't install Node.js/npm (system-level, not Cargo's job)
- Don't install cargo-binstall (user preference)
- Don't modify system config or PATH

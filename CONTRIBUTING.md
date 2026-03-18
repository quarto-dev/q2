# Contributing to Quarto 2

> We welcome discussions about the project via GitHub issues. The Quarto team is working on this codebase internally before accepting outside contributions, but questions, suggestions, and bug reports are welcome.

## Prerequisites

### Rust toolchain

The repo includes a `rust-toolchain.toml` that auto-installs the correct nightly toolchain, components (rustfmt, clippy), and the `wasm32-unknown-unknown` target on your first `cargo` invocation. No manual `rustup` steps needed.

### Development tools (cargo-nextest, wasm-pack)

This project uses [nextest](https://nexte.st/) instead of `cargo test`, and [wasm-pack](https://rustwasm.github.io/wasm-pack/) for building WASM modules. Install both with:

```bash
cargo dev-setup
```

This detects already-installed tools and skips them. When [cargo-binstall](https://github.com/cargo-bins/cargo-binstall) is available, it downloads pre-built binaries (seconds); otherwise it falls back to `cargo install --locked` (slower, compiles from source).

### Pandoc 3.6+ (optional)

Four tests in the `pampa` crate compare output against Pandoc. These tests require Pandoc 3.6 or later and will fail when Pandoc is missing or too old. `cargo dev-setup` checks this and warns if needed.

Install from [pandoc.org/installing](https://pandoc.org/installing.html).

### Node.js and npm

Required for the hub-client web application. Any recent LTS version works.

## Building

### Rust workspace

```bash
cargo build --workspace
```

### hub-client (web client + WASM)

This project uses npm workspaces. Always run `npm install` from the **repo root**, not from `hub-client/`:

```bash
npm install              # from repo root
cd hub-client
npm run build:all        # builds WASM module + web client
```

## Testing

### Rust tests

```bash
cargo nextest run --workspace
```

Do **not** use `cargo test` — nextest is required for correct test execution in this workspace.

### Full verification

`cargo xtask verify` runs the complete verification suite: Rust build, Rust tests, hub-client WASM build, and hub-client tests.

```bash
cargo xtask verify
```

Skip options for faster iteration:

```bash
cargo xtask verify --skip-rust-tests
cargo xtask verify --skip-hub-tests
cargo xtask verify --skip-hub-build
```

### Custom lint checks

```bash
cargo xtask lint
```

## Platform Notes

### Windows

**pampa-fuzz**: Excluded from default workspace members via `default-members` in `Cargo.toml`. The `libfuzzer-sys` dependency only builds on Linux/macOS.

**v8 test compilation**: The `v8` crate does not produce an rlib on Windows, causing test compilation (not regular builds) to fail for crates that transitively depend on it. `cargo build --workspace` works fine. For tests, exclude the affected crates:

```bash
cargo nextest run --workspace \
  --exclude quarto-system-runtime \
  --exclude pampa \
  --exclude quarto-core \
  --exclude quarto-sass \
  --exclude quarto-test \
  --exclude quarto \
  --exclude quarto-project-create \
  --exclude qmd-syntax-helper \
  --exclude comrak-to-pandoc \
  --exclude quarto-lsp \
  --exclude quarto-lsp-core \
  --exclude reconcile-viewer
```

See `claude-notes/instructions/windows-dev.md` for the full details and the list of testable crates.

### macOS / Linux

No known platform-specific issues. All workspace crates build and test without exclusions.

## Editor Setup

The repo includes VS Code configuration in `.vscode/`:

- `settings.json` — rust-analyzer format-on-save
- `launch.json` — LLDB debug configurations for key crates
- `extensions.json` — recommended extensions (rust-analyzer, CodeLLDB, Even Better TOML)

## AI-Assisted Development

This repo includes a `CLAUDE.md` with project-specific instructions for [Claude Code](https://docs.anthropic.com/en/docs/claude-code). See that file for workspace structure, build commands, testing conventions, and coding guidelines.

# WASM Architecture

## Overview

The `wasm-quarto-hub-client` crate builds the Quarto rendering engine (pampa + quarto-core)
as a WASM module for use in the hub-client web application. It targets
`wasm32-unknown-unknown` and uses `-Zbuild-std=std,panic_unwind` to rebuild the standard
library (required for Lua error handling via setjmp/longjmp → panic/catch_unwind).

## Build

The WASM module is built via `hub-client/scripts/build-wasm.js`, which runs:
1. `cargo build --target wasm32-unknown-unknown -Zbuild-std=std,panic_unwind`
2. `wasm-bindgen` CLI to generate JS glue code

From hub-client:
```bash
npm run build:all    # Full build including WASM
```

This project does **not** use wasm-pack (deprecated, rustwasm sunset Sep 2025).
The `wasm-bindgen-cli` version is pinned to match `Cargo.lock` and installed via
`cargo xtask dev-setup`.

## C Toolchain

Building for `wasm32-unknown-unknown` requires Clang with wasm32 support. The `cc` crate
invokes Clang to compile C dependencies (tree-sitter, Lua). Environment variables:

```bash
CC_wasm32_unknown_unknown=clang
CFLAGS_wasm32_unknown_unknown="-isystem <path>/wasm-sysroot -fno-builtin"
```

The wasm-sysroot at `crates/wasm-quarto-hub-client/wasm-sysroot/` provides minimal C
headers. The `-fno-builtin` flag is needed because debug-mode builds emit `__builtin_*`
intrinsic calls not present in the stub sysroot.

## Native vs WASM Testing

Native tests (`cargo nextest run`) use `Lua::new()` with the full C stdlib on all platforms.
WASM-specific code paths use `#[cfg(target_arch = "wasm32")]` guards — never
`#[cfg(any(target_arch = "wasm32", test))]` (see `.claude/rules/wasm.md`).

Hub-client integration tests (`npm run test:ci`) exercise the compiled WASM module through
the JavaScript API.

## wasm-qmd-parser (dormant)

A lightweight WASM wrapper around `pampa` only, without the full `quarto-core` rendering
stack. Currently dormant — the crate skeleton is kept for future use as a smaller, faster
WASM module for contexts that only need parsing (not full document rendering). It will need
updates to match the current `pampa` API before it can build again.

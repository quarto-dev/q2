# WASM in the Quarto Rust Monorepo

## Architecture

`wasm-quarto-hub-client` wraps `pampa` + `quarto-core` for the hub-client web app.
It compiles to a WASM module that runs in the browser, providing live preview rendering.

The crate is **excluded from the default workspace** (`Cargo.toml` `exclude` list) because
it requires `--target wasm32-unknown-unknown` and `-Zbuild-std=std,panic_unwind`.

## Build

The production WASM build is handled by `hub-client/scripts/build-wasm.js`:

```bash
cd hub-client
npm run build:wasm    # WASM module only
npm run build:all     # WASM + TypeScript
```

The build script runs:
1. `cargo build -p wasm-quarto-hub-client --target wasm32-unknown-unknown`
   with `-Zbuild-std=std,panic_unwind` (via `crates/wasm-quarto-hub-client/.cargo/config.toml`)
2. `wasm-bindgen` CLI to generate JS/TS bindings

### Why not wasm-pack?

This project uses `cargo build` + `wasm-bindgen` CLI directly because:
- `-Zbuild-std=std,panic_unwind` is required for Lua error handling (setjmp/longjmp to
  panic/catch_unwind). wasm-pack doesn't support `-Zbuild-std`.
- wasm-pack is deprecated (rustwasm org sunset September 2025).

### C toolchain requirement

`pampa` with `lua-filter` pulls in `mlua` → `lua-src-wasm`, which compiles Lua from C source
via the `cc` crate. When targeting wasm32, this requires Clang with wasm32 support:

```bash
export CC_wasm32_unknown_unknown=clang
export CFLAGS_wasm32_unknown_unknown="-isystem $PWD/crates/wasm-quarto-hub-client/wasm-sysroot -fno-builtin"
```

Production builds (`build-wasm.js`) only set `-isystem <wasm-sysroot>`. WASM tests
additionally need `-fno-builtin` because they compile in debug mode, where Clang emits
`__builtin_*` intrinsic calls (e.g. `memcpy`, `memset`) that don't exist in the stub
sysroot. Release builds inline or eliminate these calls, so the flag isn't needed there.

## Testing

### Native tests (all platforms)

Native Rust tests (`cargo nextest run`) test filter and shortcode logic using `Lua::new()`
with the full C stdlib. These run on all platforms including Windows.

### WASM smoke tests (Linux CI)

`crates/pampa/tests/wasm_lua.rs` contains smoke tests that compile and run on the real
`wasm32-unknown-unknown` target. They verify the WASM-specific Lua VM setup:
- Restricted stdlib creation (`Lua::new_with()`)
- Synthetic `io`/`os` module registration
- Filter execution through the WASM code path
- Shortcode engine initialization on WASM
- Error handling (`panic_unwind` works correctly)

Run locally (Linux/macOS with LLVM):
```bash
CC_wasm32_unknown_unknown=clang \
CFLAGS_wasm32_unknown_unknown="-isystem $PWD/crates/wasm-quarto-hub-client/wasm-sysroot -fno-builtin" \
cargo test -p pampa --test wasm_lua --target wasm32-unknown-unknown \
  --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind,panic_abort
```

**Important notes:**

- You must use `--test wasm_lua` to select only the WASM test file.
  Running `cargo test -p pampa --target wasm32` without `--test` will fail because
  native tests can't compile for wasm32.
- The prebuilt `wasm32-unknown-unknown` target (installed by `rust-toolchain.toml`)
  conflicts with `-Zbuild-std` when building within the workspace — both produce a
  `core` crate, causing E0152 (duplicate lang item). The production build avoids this
  because `wasm-quarto-hub-client` is excluded from the workspace. The CI job sets
  `RUSTUP_TOOLCHAIN=nightly` to bypass `rust-toolchain.toml`, so the prebuilt target
  is never installed. Locally, you can either set `RUSTUP_TOOLCHAIN=nightly` or
  remove the target before testing (`rustup target remove wasm32-unknown-unknown`)
  and re-add it afterward for the production build.
- The pampa `[[bin]]` targets (`pampa`, `ast-reconcile`) use `required-features` to
  prevent compilation when running WASM tests. Cargo builds bin targets alongside
  integration tests by default (rust-lang/cargo#12980); the `required-features` gate
  ensures they are skipped when `--no-default-features --features lua-filter` is used.

WASM tests are **not** part of `cargo xtask verify` — they require nightly + Clang with
wasm32 support, which is Linux/macOS only. They run in the `wasm-tests` CI job.

### Hub-client integration tests

The hub-client test suite (`npm run test:ci`) tests the compiled WASM module through
JavaScript, covering rendering, templates, and format detection. These complement
the Rust-level WASM smoke tests.

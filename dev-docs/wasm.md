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

### C stdlib shims (`wasm-c-shim`)

`wasm32-unknown-unknown` has no libc. C libraries (tree-sitter, Lua) that reference
standard symbols (`malloc`, `fprintf`, `snprintf`, `abort`, etc.) need Rust-provided
`#[no_mangle]` shim functions at link time.

These shims live in `crates/wasm-c-shim/`, a workspace member that is a no-op on native
targets (all exports gated on `cfg(target_arch = "wasm32")`). Both `wasm-quarto-hub-client`
(production) and `pampa` WASM tests (dev-dependency) link against it.

The crate also replaces Lua's `LUAI_THROW`/`LUAI_TRY` macros (normally `setjmp`/`longjmp`)
with `panic!()` / `catch_unwind`, since wasm32 has no native unwinding. The panic payload
is `wasm_c_shim::LuaThrow`, a public marker type. Hosts that install a custom panic hook
(e.g. `wasm-quarto-hub-client`'s `init()`) can downcast to `LuaThrow` to filter expected
Lua control-flow panics out of `console.error` without suppressing real Rust panics.

**Edition note:** `wasm-c-shim` uses edition 2021, not the workspace default of 2024.
Edition 2024 requires explicit `unsafe {}` blocks inside `unsafe fn`, which would add
noise to ~65 FFI shim functions with no safety benefit.

### Wasm32 panic strategy and rustflags

The `wasm-c-shim` `panic`/`catch_unwind` substitution only works when the binary's panic
strategy is `unwind`. The wasm32-unknown-unknown default is `abort`, under which `panic!()`
lowers to the wasm `unreachable` instruction and `catch_unwind` becomes a compile-time
no-op — meaning the first Lua throw during mlua initialization aborts the whole module.

Three flags must be set on every wasm32 build that touches `wasm-c-shim`:

```
-C target-feature=+bulk-memory,+exception-handling
-C panic=unwind
-Zwasm-c-abi=spec
```

These live in two `.cargo/config.toml` files so they apply both to the production build
and to wasm32 invocations from the workspace root:

- `crates/wasm-quarto-hub-client/.cargo/config.toml` — used when `build-wasm.js` builds
  the production cdylib from the isolated hub-client workspace.
- `.cargo/config.toml` (workspace root) — used by `cargo test --target wasm32-unknown-unknown`
  invocations from the monorepo root, including the `pampa wasm_lua` tests.

`[unstable] build-std` is **not** in the workspace-root config because the `[unstable]` table
is not target-scoped — adding it would force `build-std` for every native invocation. The
`-Zbuild-std` flag stays on the test command and in CI.

### JS bridge feature gate (`quarto-system-runtime`)

`quarto-system-runtime/src/wasm.rs` declares four
`#[wasm_bindgen(raw_module = "/src/wasm-js-bridge/{template,sass,cache,fetch}.js")]`
extern blocks. Hub-client serves these JS modules at runtime through Vite, but
`wasm-bindgen` generates unconditional `require()` calls for the absolute paths in the
JS shim it produces. Under Node.js (where `wasm-bindgen-test-runner` runs), the paths do
not resolve and module load fails with `MODULE_NOT_FOUND`.

To keep test wasm builds loadable, the four extern blocks are gated behind a
`js-bridge` Cargo feature on `quarto-system-runtime` (default off). When the feature
is off, stub modules return `Err(JsValue::from_str("js-bridge feature not enabled"))` or
`false`, preserving the `SystemRuntime` impl. `wasm-quarto-hub-client/Cargo.toml` opts in:

```toml
quarto-system-runtime = { path = "../quarto-system-runtime", features = ["js-bridge"] }
```

Pampa's wasm test build does not, so the `require()` calls disappear from the generated shim.

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
  --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind
```

The `-C panic=unwind`, `+exception-handling`, and `-Zwasm-c-abi=spec` flags are picked up
automatically from the workspace-root `.cargo/config.toml` — see "Wasm32 panic strategy
and rustflags" above. Only `panic_unwind` is needed in `-Zbuild-std` because the binary
uses the unwind strategy; `panic_abort` is unused.

On macOS, Apple's bundled clang does not include the wasm32 target. Use Homebrew LLVM
instead: `CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/clang`.

**Important notes:**

- You must use `--test wasm_lua` to select only the WASM test file.
  Running `cargo test -p pampa --target wasm32` without `--test` will fail because
  native tests can't compile for wasm32.
- The pinned toolchain in `rust-toolchain.toml` (a nightly with `rust-src` and the
  wasm32 target) is what both CI and local runs should use — no `RUSTUP_TOOLCHAIN`
  override. Do not substitute a newer nightly: bd-at72 pinned the toolchain because
  later nightlies SIGSEGV in LLVM ThinLTO when building this workspace for wasm32.
  (An earlier iteration of this setup hit E0152 duplicate-lang-item conflicts
  between the prebuilt wasm32 sysroot and `-Zbuild-std`; that no longer reproduces
  on the pinned toolchain with the prebuilt target installed.)
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

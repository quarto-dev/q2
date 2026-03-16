# Design: Lua Filters on wasm32-unknown-unknown

**Date**: 2026-03-18
**Branch**: `experiment/lua-wasm`
**Worktree**: `~/src/q2-lua-wasm-spike`
**Status**: Proven — end-to-end Lua filters work in the browser

---

## Overview

Enable Lua filters in the hub-client WASM build by compiling PUC-Rio Lua 5.4
(via mlua) for `wasm32-unknown-unknown`. The core challenge is that Lua uses
`setjmp`/`longjmp` for error handling, which don't exist on bare WASM. We
replace them with Rust's `panic!`/`catch_unwind`, using WASM exception handling
instructions to propagate panics through C frames.

---

## Context

### Current state

The hub-client renders Quarto documents in the browser via a WASM build of the
rendering engine (`wasm-quarto-hub-client`). This build targets
`wasm32-unknown-unknown` with `wasm-bindgen`. C code (tree-sitter parsers)
already compiles to WASM using Homebrew LLVM clang + a stub sysroot + Rust libc
shims in `c_shim.rs`.

The Lua filter engine in `pampa/src/lua/` (~24,600 lines) uses mlua (Rust
bindings to Lua 5.4 via C FFI). It's gated behind a `lua-filter` cargo feature,
which was disabled for the WASM build because Lua's C implementation requires
`setjmp`/`longjmp`.

### Why this matters

Lua filters are a core Quarto feature. Without them, the hub-client preview
can't show filtered output, meaning users can't see their actual rendered
document while editing.

---

## Architecture

```
hub-client (browser)
      |
  wasm-bindgen
      |
wasm-quarto-hub-client        (wasm32-unknown-unknown)
      |
  quarto-core
      |
    pampa                      (lua-filter feature ON)
   /     \
mlua    (rest of pampa)
  |
mlua-sys
  |
lua-src-wasm                   forked lua-src, wasm32 build path
  |
Lua 5.4.8 C source            compiled by cc crate + Homebrew LLVM
  |
luaconf_wasm.h                 overrides LUAI_TRY/LUAI_THROW
  |
c_shim.rs                      libc stubs + catch_unwind/panic shims
```

### Error handling mechanism

Lua's error handling is defined by three macros in `ldo.c`:

- `LUAI_THROW(L,c)` — default: `longjmp((c)->b, 1)`
- `LUAI_TRY(L,c,a)` — default: `if (setjmp((c)->b) == 0) { a }`
- `luai_jmpbuf` — default: `jmp_buf`

These are guarded by `#if !defined(LUAI_THROW)`, so pre-defining them in a
force-included header completely replaces the default implementation. The C++
path already does this (uses `throw`/`catch`), so the mechanism is well-tested
upstream.

Our replacement (`luaconf_wasm.h`):

```c
#define luai_jmpbuf     int                      /* dummy, like C++ path */
#define LUAI_THROW(L,c) rust_lua_throw()
#define LUAI_TRY(L,c,a) \
    if (rust_lua_protected_call(f, L, ud) != 0) { \
        if ((c)->status == 0) (c)->status = -1;    \
    }
```

Rust side (`c_shim.rs`):

```rust
#[no_mangle]
pub extern "C-unwind" fn rust_lua_protected_call(
    f: extern "C-unwind" fn(*mut c_void, *mut c_void),
    l: *mut c_void,
    ud: *mut c_void,
) -> i32 {
    match std::panic::catch_unwind(AssertUnwindSafe(|| f(l, ud))) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

#[no_mangle]
pub extern "C-unwind" fn rust_lua_throw() -> ! {
    panic!("lua error");
}
```

This works because:

1. `panic=unwind` is enabled for the WASM target (via `-Zbuild-std`)
2. WASM exception handling instructions (`+exception-handling` target feature)
   allow panics to propagate through C frames
3. mlua uses `extern "C-unwind"` for all FFI declarations, so unwinding across
   the Rust→C→Rust boundary is defined behavior
4. `catch_unwind` at the `LUAI_TRY` boundary catches the panic, exactly like
   `setjmp` would catch a `longjmp`

### Build requirements

| Requirement | Notes |
|------------|-------|
| Nightly Rust | Needed for `-Zbuild-std` |
| `rust-src` component | `rustup component add rust-src` |
| `-Zbuild-std=std,panic_unwind` | Rebuilds std with panic=unwind for WASM |
| `-Cpanic=unwind` | Override default `abort` for wasm32 |
| `-Ctarget-feature=+exception-handling` | Enable WASM EH instructions |
| Homebrew LLVM clang | Apple clang doesn't support wasm32 target |
| `wasm-bindgen` CLI | `wasm-pack` doesn't support `-Zbuild-std` |

Browser support for WASM exception handling: Chrome 95+, Firefox 100+,
Safari 15.2+. This covers essentially all modern browsers.

### Build command

```bash
cd crates/wasm-quarto-hub-client

CC_wasm32_unknown_unknown="/opt/homebrew/opt/llvm/bin/clang" \
CFLAGS_wasm32_unknown_unknown="-isystem $(pwd)/wasm-sysroot" \
cargo build --target wasm32-unknown-unknown --release

wasm-bindgen --target web --out-dir pkg \
  target/wasm32-unknown-unknown/release/wasm_quarto_hub_client.wasm
```

This replaces `wasm-pack` because wasm-pack doesn't support `-Zbuild-std`.
The `.cargo/config.toml` in the crate provides the nightly flags.

---

## Components

### 1. `crates/lua-src-wasm/` — Forked lua-src

A fork of `lua-src` (the crate that compiles Lua C source via the `cc` crate).
The only meaningful change is a `wasm32-unknown-unknown` match arm in the build
script and the `luaconf_wasm.h` force-include.

**Why fork instead of upstream?** The wasm32 build path has fundamentally
different error handling (panic vs setjmp). This is unlikely to be accepted
upstream without significant discussion. A local fork keeps the experiment
moving.

**Cleanup path**: Contribute the wasm32 support upstream to `lua-src-rs`, or
maintain as a `[patch.crates-io]` indefinitely. The patch is small (~50 lines
in the build script + 29-line header).

### 2. `crates/wasm-bindgen-futures-patch/` — Patched wasm-bindgen-futures

Removes the `UnwindSafe` bound from `future_to_promise`. With `panic=unwind`,
many types (including mlua's Lua state) aren't `UnwindSafe`, but we need them
in async contexts. The patch wraps the future in `AssertUnwindSafe` instead.

**Cleanup path**: This is a known ergonomic issue. Either upstream accepts
the change, or we find a way to wrap only the Lua-touching futures. For
production, we should evaluate whether `AssertUnwindSafe` is actually safe
in our usage (it is — Lua errors are caught at the `LUAI_TRY` boundary before
reaching the future).

### 3. `c_shim.rs` — Libc stubs for Lua

The existing `c_shim.rs` (which provides libc functions for tree-sitter) was
extended with everything Lua needs. All implementations are hand-written Rust.

**Functions added for Lua**:

- **Core shims**: `rust_lua_protected_call`, `rust_lua_throw`
- **String**: `strlen`, `strcmp`, `strchr`, `strcpy`, `memchr`, `strpbrk`,
  `strspn`, `strcoll`, `strerror`
- **Ctype**: `isdigit`, `isalpha`, `isalnum`, `isspace`, `isupper`, `islower`,
  `iscntrl`, `ispunct`, `isgraph`, `isxdigit`, `toupper`, `tolower`
- **Stdlib**: `abs`, `strtod` (full implementation including hex floats)
- **Math**: `frexp`
- **Locale**: `localeconv` (stub returning `"."`)
- **Errno**: `__errno_location` (static int)
- **Time**: `time` (returns 42), `clock` (returns 0) — must return `i32`/`u32`
  to match wasm32 ABI where `long` is 32-bit
- **Stdio stubs**: `fopen`, `freopen`, `fgets`, `fread`, `fflush`, `ferror`,
  `feof`, `getc` — all return error/empty since WASM has no filesystem
- **Lua library stubs**: `luaopen_io`, `luaopen_os`, `luaopen_package` — needed
  because `linit.c` references them even when not loaded

**Cleanup path**: Consider using `tinyrlibc` or `libm` crates for the math and
string functions. For an experiment, hand-writing them avoids dependency
management. For production, well-tested crate implementations would be better
for the more complex functions (especially `strtod`).

### 4. `wasm-sysroot/` — Extended stub C headers

Added headers that Lua's C source includes: `ctype.h`, `errno.h`, `float.h`,
`limits.h`, `locale.h`, `math.h`, `setjmp.h`, `signal.h`, `time.h`. Extended
existing `stdio.h`, `stdlib.h`, `string.h`.

These declare the functions that `c_shim.rs` implements. They must be provided
via `-isystem` so they apply to all C compilation (Lua, tree-sitter, etc.).

### 5. Lua library restrictions on WASM

`Lua::new()` fails on WASM because it tries to disable `package.loadlib`, but
our `luaopen_package` stub is empty (returns nil). Instead, we use:

```rust
#[cfg(target_arch = "wasm32")]
let lua = {
    use mlua::StdLib;
    let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING
             | StdLib::UTF8 | StdLib::MATH;
    Lua::new_with(libs, LuaOptions::default())?
};
```

**Available**: base (implicit), coroutine, table, string, utf8, math
**Unavailable**: io, os, package (dynamic loading), debug (mlua rejects in safe mode)

This is sufficient for Quarto's Lua filter API — filters use `pandoc.*`
constructors (registered by our Rust code) and standard string/table/math
operations.

### 6. SystemRuntime threading for VFS-based filter reading

The Lua filter engine previously used `std::fs::read_to_string()` to read
filter files. On WASM, there's no filesystem — files live in the VFS. The fix
threads `Arc<dyn SystemRuntime>` through the entire call chain:

```
render_qmd_to_html(runtime)
  → run_pipeline(runtime)
    → UserFiltersStage::run(ctx.runtime)
      → apply_filters(runtime)
        → apply_lua_filter(runtime)
          → runtime.file_read(filter_path)
```

On native, `NativeRuntime` delegates to `std::fs`. On WASM, `WasmRuntime`
reads from the VFS. The pipeline resolves relative filter paths against the
document directory, and the VFS normalizes them to `/project/` prefix.

---

## What works

Verified across three test runtimes:

| Runtime | Tests | Result |
|---------|-------|--------|
| Native Rust (`cargo nextest run`) | smoke-all filter fixtures | Pass |
| WASM Vitest (`npm run test:wasm`) | 37 fixtures including filters | 37/37 pass |
| Playwright E2E (`--workers=1`) | 37 fixtures including filters | 37/37 pass |
| Node.js standalone (`test-lua-wasm.mjs`) | 10 Lua tests | 10/10 pass |

Tested Lua features: string operations, integer/float math, `string.format`,
table sort, `pcall` error handling, coroutines, `pandoc.*` constructors,
filter traversal (Str, Para, etc.), AST transformation (uppercasing), nested
pcall, and end-to-end filter through the full render pipeline.

---

## What doesn't work / Known limitations

1. **No `io`/`os`/`package` libraries**: Filters that read files, run commands,
   or `require()` modules will fail. This is inherent to the browser sandbox.
   Most Quarto filters don't need these.

2. **`time()` returns a constant (42)**: Filters using `os.time()` or
   `os.clock()` get fake values. This is fine for filters but would break
   benchmarking code.

3. **`strtod` is hand-written**: The hex float parsing implementation works
   but hasn't been fuzz-tested. Edge cases in float formatting (`string.format`
   with `%a`, `%g`) may have minor differences from glibc.

4. **`snprintf` is incomplete**: The existing implementation (from tree-sitter
   support) handles `%s`, `%d`, `%f`, `%x`, `%p`, `%c`, `%e`, `%g` but may
   miss edge cases. Lua's `string.format` relies on the C `snprintf`, so
   unusual format strings may produce wrong output.

5. **No `debug` library**: mlua rejects it in safe mode. Filters using
   `debug.getinfo()` or similar will fail.

6. **Parallel worker flakiness in Playwright**: With multiple workers,
   SASS compilation + WASM rendering can cause timeouts. CI uses retries
   to mask this. `--workers=1` is fully reliable.

---

## Cleanup for production

### Must do

- **Audit `AssertUnwindSafe` usage**: Verify that Lua errors caught by
  `catch_unwind` don't leave the Lua VM in a corrupted state. In practice
  this should be fine — Lua's own error handling is designed to be caught at
  `LUAI_TRY` boundaries — but it needs a careful review.

- **Test with real-world filters**: Run the full Quarto filter test suite
  and popular community filters (e.g., `quarto-ext/lightbox`, `schochastics/academicons`).

- **Error reporting**: Currently, Lua filter errors that propagate as panics
  are caught but the error message is generic (`"lua error"`). The actual Lua
  error message should be preserved and surfaced to the user.

### Should do

- **Replace hand-written libc with crates**: Use `tinyrlibc` for string
  functions, `libm` for math. Reduces risk of bugs in `strtod`, `frexp`, etc.

- **Upstream lua-src changes**: The wasm32 build path is small and clean.
  Contributing it to `mlua-rs/lua-src-rs` would eliminate the fork.

- **Upstream wasm-bindgen-futures fix**: The `UnwindSafe` bound removal is
  a reasonable change for the `panic=unwind` WASM use case.

- **Binary size**: The WASM binary grew from ~10MB to ~14MB with Lua. Investigate
  what can be trimmed (e.g., `wasm-opt`, LTO, excluding unused Lua source files).

### Nice to have

- **Partial `io` library**: Could implement `io.read`/`io.write` backed by
  VFS for filters that read auxiliary data files.

- **`require()` support**: Could implement a VFS-backed module loader so
  filters can `require` other Lua files from the project.

- **Streaming compilation**: Lua source is compiled to bytecode on every render.
  Could cache compiled bytecode in the VFS for frequently-used filters.

---

## Key learnings

1. **WASM type signatures are strict**: If C expects `long` (32-bit on wasm32)
   but Rust returns `i64`, WASM traps with `signature_mismatch` at runtime.
   There's no implicit conversion — types must match exactly.

2. **`extern "C-unwind"` is critical**: Panics must propagate through C frames.
   mlua already uses `C-unwind` for all FFI declarations. Our shims must match.

3. **Apple clang can't target wasm32**: Must use Homebrew LLVM. The `CC_wasm32_unknown_unknown`
   env var makes this transparent to the build.

4. **`wasm-pack` doesn't support `-Zbuild-std`**: Must use `cargo build` +
   `wasm-bindgen` CLI directly. `build-wasm.js` was updated accordingly.

5. **`Lua::new()` vs `Lua::new_with()`**: On WASM, `Lua::new()` tries to
   disable C module loading and fails because our `luaopen_package` stub is
   empty. Using `Lua::new_with()` with an explicit library set avoids this
   entirely and is actually more correct for the sandbox.

6. **Global CFLAGS needed**: The `-isystem` flag for the stub sysroot must
   apply to ALL C compilation (tree-sitter, lua-src, etc.), not just lua-src.
   `CFLAGS_wasm32_unknown_unknown` achieves this.

# Experiment: Lua on wasm32-unknown-unknown via LUAI_TRY/LUAI_THROW Override

**Date**: 2026-03-16
**Branch**: `experiment/lua-wasm`
**Worktree**: `~/src/q2-lua-wasm-spike` (git worktree of `~/src/q2`)
**Status**: Phase 4 COMPLETE — End-to-end Lua filter works through full render pipeline! 10/10 tests pass.
**Context**: [Investigation](../investigations/2026-03-16-lua-wasm-options.md) — Option 8

## Goal

Prove that we can compile mlua + PUC-Rio Lua 5.4 for `wasm32-unknown-unknown`
and run a real Lua filter in the hub-client WASM build. This is an experiment —
right architecture, not a perfectly clean implementation. Demo target: work week.

---

## Context for Fresh Agents

This is a Rust monorepo (Quarto) where the hub-client web app uses a WASM build
of the rendering engine. The WASM build targets `wasm32-unknown-unknown` (bare,
no libc). The experiment adds Lua scripting support to the WASM build.

### Key files in this worktree

- `crates/wasm-quarto-hub-client/` — The WASM crate (excluded from workspace, has own Cargo.toml)
  - `src/c_shim.rs` — Rust implementations of C libc functions for WASM (malloc, strlen, etc.)
  - `src/lib.rs` — WASM entry points, includes `test_lua()` and `test_unwind()` functions
  - `wasm-sysroot/` — Stub C headers for compilation
  - `.cargo/config.toml` — Build flags including `-Zbuild-std` and `+exception-handling`
  - `Cargo.toml` — Has `[patch.crates-io]` for lua-src and wasm-bindgen-futures
  - `test-lua-wasm.mjs` — Node.js test script that patches JS glue and runs Lua tests
- `crates/lua-src-wasm/` — Forked lua-src with wasm32-unknown-unknown support
  - `lua-5.4.8/luaconf_wasm.h` — Overrides LUAI_TRY/LUAI_THROW to use Rust catch_unwind/panic
  - `src/lib.rs` — Build script with wasm32 match arm
- `crates/wasm-bindgen-futures-patch/` — Patched wasm-bindgen-futures 0.4.58
  - Removes `UnwindSafe` bound from `future_to_promise` (needed for panic=unwind compat)
- `crates/pampa/src/lib.rs` — Has `lua_wasm_test()` function that creates Lua VM and evals script
- `test-unwind/` — Minimal standalone test proving catch_unwind works on wasm32

### How to build

```bash
cd crates/wasm-quarto-hub-client

# Build WASM (requires nightly Rust with rust-src component)
CC_wasm32_unknown_unknown="/opt/homebrew/opt/llvm/bin/clang" \
CFLAGS_wasm32_unknown_unknown="-isystem $(pwd)/wasm-sysroot" \
cargo build --target wasm32-unknown-unknown --release

# Generate JS glue (web target for ESM compatibility)
wasm-bindgen --target web --out-dir pkg target/wasm32-unknown-unknown/release/wasm_quarto_hub_client.wasm
```

**Important**: Must use Homebrew LLVM clang (not Apple clang) because Apple clang
doesn't support the `wasm32-unknown-unknown` target. The `-isystem` flag provides
our stub sysroot headers to all C compilation (tree-sitter, lua-src, etc.).

### How to test

```bash
cd crates/wasm-quarto-hub-client
node test-lua-wasm.mjs
```

The test script patches out hub-client JS bridge imports and runs Lua scripts through WASM.
Expected output: 8 passed, 0 failed. The `panicked at src/c_shim.rs:452:5: lua error`
messages on stderr are EXPECTED — that's Lua's error handling mechanism (throw via panic,
caught by `catch_unwind` in `rust_lua_protected_call`).

---

## Background: The Problem

Our hub-client is a browser app that uses a WASM build of the Quarto rendering
engine for live preview. The WASM build targets `wasm32-unknown-unknown` with
`wasm-bindgen` (the standard Rust-in-browser toolchain).

Quarto supports **Lua filters** — user scripts that transform the document AST.
The filter engine lives in `crates/pampa/src/lua/` (~24,600 lines) and is built
on **mlua** (Rust bindings to PUC-Rio Lua 5.4 via C FFI). It's gated behind a
`lua-filter` cargo feature.

The WASM build currently disables `lua-filter` because Lua's C implementation
uses `setjmp`/`longjmp` for error handling, and `wasm32-unknown-unknown` has
**no libc, no setjmp, no longjmp** — it's a bare execution environment.

This experiment overrides Lua's error handling to use Rust's `panic!`/`catch_unwind`
instead, which DO work on WASM with the right compiler flags.

## Background: The Codebase

### Workspace structure (relevant crates)

```
crates/
  pampa/                     # Core Quarto engine
    src/lua/                 # Lua filter engine (~24,600 LOC on mlua)
      filter.rs              # Traversal engine (typewise/topdown)
      types.rs               # AST <-> Lua UserData marshalling
      constructors.rs        # ~30+ pandoc.* element constructors
      list.rs                # pandoc.List metatable
      utils.rs               # pandoc.utils.* namespace
      ...                    # 14 files total
    Cargo.toml               # lua-filter = ["dep:mlua"] feature flag
  quarto-core/               # Higher-level orchestration
    src/pipeline.rs          # Native pipeline has UserFiltersStage; WASM pipeline does NOT
  wasm-quarto-hub-client/    # WASM crate for hub-client
    src/c_shim.rs            # Rust implementations of libc functions for C code in WASM
    src/lib.rs               # Entry point, includes c_shim and test_lua()
    wasm-sysroot/            # Stub C headers (stdio.h, stdlib.h, string.h, etc.)
    Cargo.toml               # pampa with lua-filter enabled, patches for lua-src and wasm-bindgen-futures
  wasm-qmd-parser/           # Older/lighter WASM crate (also has c_shim + wasm-sysroot)
  lua-src-wasm/              # FORKED lua-src with wasm32-unknown-unknown support
  wasm-bindgen-futures-patch/ # Patched wasm-bindgen-futures (UnwindSafe bound removed)
hub-client/
  scripts/build-wasm.js      # Builds WASM via wasm-pack, sets CFLAGS for C compilation
```

### How the WASM build works today

The hub-client's WASM module is built by `hub-client/scripts/build-wasm.js`:
1. Sets `CC_wasm32_unknown_unknown` to homebrew LLVM clang
2. Sets `CFLAGS_wasm32_unknown_unknown="-I{wasm-sysroot} -fno-builtin -DHAVE_ENDIAN_H"`
3. Runs `wasm-pack build --target web` on `crates/wasm-quarto-hub-client`

C code (currently just tree-sitter parsers) compiles to WASM using the
`cc` crate + homebrew LLVM (`/opt/homebrew/opt/llvm/bin/clang`), which supports
the `wasm32-unknown-unknown` target. Missing libc functions are provided by
Rust implementations in `c_shim.rs` (malloc, free, memcpy, snprintf, etc.)
with corresponding header stubs in `wasm-sysroot/`.

### How mlua and lua-src work

- **mlua** (v0.11): Rust bindings to Lua. Crate at `~/src/mlua/` (local checkout).
  - `mlua-sys/src/lua54/`: FFI declarations — all use `extern "C-unwind"` (critical!)
  - `mlua-sys/build/find_vendored.rs`: calls `lua_src::Build::new().build(lua_src::Lua54)`
- **lua-src** (v550.0.0): Compiles Lua C source via `cc` crate.
  - Source repo: `~/src/lua-src-rs/` (clone of https://github.com/mlua-rs/lua-src-rs)
  - `src/lib.rs`: Build script with target-matching chain (linux/mac/windows/emscripten/wasi)
  - **`wasm32-unknown-unknown` is NOT supported** — falls through to error at line 210
  - `lua-5.4.8/`: The actual Lua C source code
  - `lua-5.4.8/ldo.c`: Error handling — `LUAI_TRY`/`LUAI_THROW` macros (lines 48-79)

### Feature gating

- `pampa/Cargo.toml`: `lua-filter = ["dep:mlua"]`
- `crates/quarto/Cargo.toml` (CLI binary): enables `lua-filter`
- `crates/wasm-quarto-hub-client/Cargo.toml`: NOW enables `lua-filter` (was disabled)
- `pampa/src/unified_filter.rs`: `FilterSpec::Lua` arm gated on `#[cfg(feature = "lua-filter")]`
- `quarto-core/src/pipeline.rs`: WASM pipeline omits `UserFiltersStage`

---

## Architecture

```
                    hub-client (browser)
                          |
                    wasm-bindgen
                          |
                wasm-quarto-hub-client   (wasm32-unknown-unknown)
                          |
                      quarto-core
                          |
                        pampa          (with lua-filter feature ON)
                       /     \
                    mlua    (rest of pampa)
                      |
                  mlua-sys
                      |
               lua-src (FORKED)         ← new wasm32-unknown-unknown build path
                      |
              Lua 5.4.8 C source       ← compiled by cc crate + homebrew LLVM
                      |
           wasm-sysroot headers         ← extended with Lua's needs
                      |
           c_shim.rs (Rust impls)       ← extended with Lua's needs
                      |
        rust_lua_try / rust_lua_throw   ← catch_unwind/panic shims
```

### Key Mechanism: Replacing setjmp/longjmp

Lua's error handling in `ldo.c` (lines 48-79) uses two macros:
- `LUAI_THROW(L,c)` — by default expands to `longjmp((c)->b, 1)`
- `LUAI_TRY(L,c,a)` — by default expands to `if (setjmp((c)->b) == 0) { a }`
- `luai_jmpbuf` — by default is `jmp_buf`

These are guarded by `#if !defined(LUAI_THROW)`, so pre-defining them skips
the defaults entirely. The C++ path already does this (uses `throw`/`catch`).

We override them via `luaconf_wasm.h` (force-included at build time):

```c
#define luai_jmpbuf     int  /* dummy, like C++ path */
#define LUAI_THROW(L,c) rust_lua_throw()
#define LUAI_TRY(L,c,a) \
    if (rust_lua_protected_call(f, L, ud) != 0) { \
        if ((c)->status == 0) (c)->status = -1; \
    }
```

Rust side (in `c_shim.rs`):
```rust
#[no_mangle]
pub extern "C-unwind" fn rust_lua_protected_call(
    f: extern "C-unwind" fn(*mut c_void, *mut c_void),
    l: *mut c_void,
    ud: *mut c_void,
) -> i32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(l, ud))) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

#[no_mangle]
pub extern "C-unwind" fn rust_lua_throw() -> ! {
    panic!("lua error");
}
```

### Build Requirements

| Requirement | Status | Notes |
|------------|--------|-------|
| Nightly Rust | Using (1.96) | Needed for `-Zbuild-std` |
| `rust-src` component | Installed | `rustup component add rust-src` |
| `-Zbuild-std=std,panic_unwind` | In .cargo/config.toml | Rebuilds std with panic=unwind for WASM |
| `-Cpanic=unwind` | In .cargo/config.toml | Default is `abort` for wasm32 |
| `-Ctarget-feature=+exception-handling` | In .cargo/config.toml | Enables WASM EH instructions |
| Homebrew LLVM | Installed at `/opt/homebrew/opt/llvm/bin/clang` | Supports wasm32 target |
| `CC_wasm32_unknown_unknown` | Set at build time | Point to homebrew clang |
| `CFLAGS_wasm32_unknown_unknown` | `-isystem $(pwd)/wasm-sysroot` | Provides C headers for all C deps |

---

## Work Items

### Phase 1: Fork lua-src and add wasm32-unknown-unknown build path ✅

Source: `~/src/lua-src-rs/` (clone of https://github.com/mlua-rs/lua-src-rs)

- [x] Create `crates/lua-src-wasm/` — copy from `~/src/lua-src-rs/`
- [x] Modify `src/lib.rs` build script to add `wasm32-unknown-unknown` match arm
- [x] Add `[patch.crates-io]` in workspace `Cargo.toml` AND `wasm-quarto-hub-client/Cargo.toml`
- [x] Verify the C compilation succeeds
- [x] Move worktree to `~/src/q2-lua-wasm-spike` (was under `q2/.claude/worktrees/`)
- [x] Enable `lua-filter` feature in `wasm-quarto-hub-client/Cargo.toml`

### Phase 2: Extend wasm-sysroot for Lua's needs ✅

**DONE** — All 39 unresolved symbols resolved. We hand-wrote everything in c_shim.rs
rather than using the tinyrlibc/libm/lexical-core dependency strategy from the original plan.
This was simpler and avoids dependency management complexity for an experiment.

Functions added to c_shim.rs:
- [x] `rust_lua_protected_call` and `rust_lua_throw` (core catch_unwind mechanism)
- [x] `luaopen_io`, `luaopen_os`, `luaopen_package` (stubs — linit.c references them)
- [x] String: `strlen`, `strcmp`, `strchr`, `strcpy`, `memchr`, `strpbrk`, `strspn`, `strcoll`, `strerror`
- [x] Ctype: `isdigit`, `isalpha`, `isalnum`, `isspace`, `isupper`, `islower`, `iscntrl`, `ispunct`, `isgraph`, `isxdigit`, `toupper`, `tolower`
- [x] Stdlib: `abs`, `strtod` (full implementation with hex float support)
- [x] Math: `frexp`
- [x] Locale: `localeconv` (stub returning `"."`)
- [x] Errno: `__errno_location` (static int)
- [x] Time: `time` (returns 42, `i32` — matches wasm32 `long`), `clock` (returns 0, `u32`)
- [x] Stdio: `fopen`, `freopen`, `fgets`, `fread`, `fflush`, `ferror`, `feof`, `getc` (stubs)

Verification: `env` imports in WASM binary went from 39 → 0.

### Phase 3: Wire mlua into the WASM build ✅

- [x] Enable `lua-filter` feature in `wasm-quarto-hub-client/Cargo.toml`
- [x] Add `lua_wasm_test()` function in `pampa/src/lib.rs` — creates Lua VM with safe libs
  - Uses `Lua::new_with()` with COROUTINE|TABLE|STRING|UTF8|MATH (no DEBUG — mlua rejects it)
- [x] Add `test_lua()` and `test_unwind()` wasm-bindgen functions in `wasm-quarto-hub-client/src/lib.rs`
- [x] WASM binary builds and links with zero unresolved symbols
- [x] WASM binary instantiates in Node.js
- [x] Patch `wasm-bindgen-futures` to remove `UnwindSafe` bound (in `crates/wasm-bindgen-futures-patch/`)
- [x] `.cargo/config.toml` configured with `build-std`, `panic=unwind`, `+exception-handling`
- [x] `cargo build --target wasm32-unknown-unknown --release` succeeds with build-std
- [x] Fix `time()` return type: was `i64`, must be `i32` (wasm32 `long` is 32-bit) — caused `signature_mismatch:time` trap
- [x] Fix `clock()` return type: was `u64`, must be `u32` (same reason)
- [x] Use `--target web` for wasm-bindgen (ESM-compatible JS glue)
- [x] Update test-lua-wasm.mjs to handle both `import` and `require()` patterns
- [x] **ALL 8 TESTS PASS**: simple string, integer math, float math, string ops, string.format, table sort, pcall error, coroutine

**Root cause of the previous runtime trap**: The `time()` function in c_shim.rs
returned `i64` but C's `time_t` is `long` which is 32-bit on wasm32. WASM enforces
strict type signature matching at the function call boundary, so the 64-bit vs 32-bit
mismatch triggered an `unreachable` (signature_mismatch) trap during `lua_newstate`.

### Phase 4: Enable UserFiltersStage in WASM pipeline ✅

- [x] In `quarto-core/src/pipeline.rs`, add `UserFiltersStage` to the WASM pipeline
- [x] Wire up VFS-based filter file reading (threaded `Arc<dyn SystemRuntime>` through entire call chain)
- [x] Use `Lua::new_with()` on WASM to avoid `Lua::new()` trying to disable C modules (our `luaopen_package` stub is empty)
- [x] Test with a simple filter — end-to-end `upper.lua` filter uppercases content in rendered HTML
- [x] Update `hub-client/scripts/build-wasm.js` to use cargo build + wasm-bindgen CLI

Changes made:
- `apply_lua_filter()`, `apply_lua_filters()` in `pampa/src/lua/filter.rs` — accept `Arc<dyn SystemRuntime>`, use `runtime.file_read()` instead of `std::fs::read_to_string()`
- `apply_filter()`, `apply_filters()` in `pampa/src/unified_filter.rs` — thread runtime through
- `UserFiltersStage::run()` in `quarto-core` — passes `ctx.runtime.clone()`
- `pampa/src/main.rs` — passes `NativeRuntime`
- All test files updated to pass `NativeRuntime`

### Phase 5: End-to-end demo

- [x] Create a test .qmd with a Lua filter (in test-lua-wasm.mjs end-to-end test)
- [ ] Build hub-client with WASM Lua support
- [ ] Run in browser and verify the filter transforms content
- [ ] Document what works, what doesn't, and what cleanup would be needed

---

## Lua Libraries for WASM

Use `Lua::new_with()` instead of `Lua::new()` to control which libraries load:

**KEEP**: base (implicit), coroutine, table, string, math, utf8
**SKIP**: io, os, package (dynamic loading), debug (mlua rejects in safe mode)

Note: `luaopen_io`, `luaopen_os`, `luaopen_package` still need stub implementations
because `linit.c` references them even when they aren't loaded.

---

## Risks and Mitigations

| Risk | Status | Notes |
|------|--------|-------|
| `wasm-pack` doesn't support `-Zbuild-std` | CONFIRMED | Use manual `cargo build` + `wasm-bindgen` CLI instead |
| `wasm-bindgen-futures` UnwindSafe bound | FIXED | Patched in `crates/wasm-bindgen-futures-patch/` |
| Nested pcall interactions with `catch_unwind` | WORKS | pcall error test passes |
| `unreachable` trap at runtime | **FIXED** | Was `time()` signature mismatch (i64 vs i32) |
| Browser WASM EH support | Unknown | Chrome 95+, Firefox 100+, Safari 15.2+ should work |
| `extern "C"` vs `extern "C-unwind"` confusion | Addressed | mlua uses C-unwind; our shims must too |
| Apple clang can't target wasm32 | CONFIRMED | Must use Homebrew LLVM clang via CC env var |

---

## Key Decisions Made

1. **Hand-wrote libc functions** instead of using tinyrlibc/libm/lexical-core crates.
   Simpler for an experiment. May need crates later for float formatting in snprintf.

2. **Patched wasm-bindgen-futures** instead of wrapping every async function.
   Removed `UnwindSafe` bound, wrapped future in `AssertUnwindSafe` instead.

3. **Use `cargo build` + `wasm-bindgen` CLI** instead of wasm-pack for the
   unwind build, because wasm-pack doesn't support `-Zbuild-std`.

4. **Keep wasm-pack for non-unwind builds** — it still works for compilation
   verification and produces smaller binaries (with wasm-opt).

5. **Use `--target web`** for wasm-bindgen instead of `--target nodejs`, because
   the test harness uses ESM (`import()`) and nodejs target generates CommonJS.

## Lessons Learned

1. **WASM type signature matching is strict**: If a C function expects `long` (32-bit
   on wasm32) but Rust returns `i64`, WASM traps with `signature_mismatch` at runtime.
   Always match C types exactly: `long` → `i32`/`c_long`, `unsigned long` → `u32`/`c_ulong`.

2. **Apple clang doesn't support wasm32-unknown-unknown**: Must use Homebrew LLVM
   clang. Set `CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/clang`.

3. **Global CFLAGS needed for all C deps**: Not just lua-src but tree-sitter etc.
   also need the sysroot headers. Use `CFLAGS_wasm32_unknown_unknown="-isystem ..."`.

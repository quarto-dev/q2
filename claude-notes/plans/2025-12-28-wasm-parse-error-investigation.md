# WASM Parse Error Detection Investigation

## STATUS: FIXED ✓

The issue has been identified and fixed. WASM now correctly detects parse errors.

## Root Cause Found

The `wasm-sysroot/stdio.h` file contains:

```c
#define sprintf(str, ...) 0
#define snprintf(str, len, ...) 0
```

These macros replace ALL snprintf calls in C code with `0` at compile time. The C preprocessor substitutes these before the compiler ever sees them, so our `c_shim.rs` implementation is never called.

This explains:
1. **snprintf calls: 0** - Macro replacement, not function calls
2. **Garbage log messages** - Buffer is uninitialized, logger receives garbage
3. **Empty S-expression** - tree-sitter's `to_sexp()` uses snprintf internally
4. **No "detect_error" seen** - Log message formatting fails

## Architecture Summary

### Build Flow
1. `wasm-pack build` compiles wasm-quarto-hub-client to WASM
2. This pulls in pampa → tree-sitter-qmd → tree-sitter C code
3. C code is compiled with clang for wasm32-unknown-unknown target
4. `CFLAGS_wasm32_unknown_unknown` includes `-I wasm-sysroot`
5. C code includes `<stdio.h>` which resolves to `wasm-sysroot/stdio.h`
6. snprintf calls are replaced with `0` by preprocessor
7. `c_shim.rs` provides `extern "C" fn snprintf` but it's never called

### Key Files
- `crates/tree-sitter-qmd/` - Grammar crate with C code (parser.c, scanner.c)
- `crates/tree-sitter-qmd/bindings/rust/build.rs` - Uses cc crate to compile C
- `crates/pampa/` - Parser with logger setup in `src/readers/qmd.rs`
- `crates/quarto-parse-errors/src/tree_sitter_log.rs` - Log observer
- `crates/wasm-quarto-hub-client/` - WASM entry point
- `crates/wasm-quarto-hub-client/src/c_shim.rs` - C stdlib implementations
- `crates/wasm-quarto-hub-client/wasm-sysroot/` - Stub C headers for WASM

## Proposed Fix

### Step 1: Fix snprintf in wasm-sysroot

Modify `crates/wasm-quarto-hub-client/wasm-sysroot/stdio.h`:

Before:
```c
#define sprintf(str, ...) 0
#define snprintf(str, len, ...) 0
```

After:
```c
int sprintf(char *str, const char *format, ...);
int snprintf(char *str, unsigned long n, const char *format, ...);
```

This allows C code to call our `c_shim.rs` implementations.

### Step 2: Rebuild and test

```bash
cd hub-client
node scripts/build-wasm.js
node test-wasm.mjs
```

Expected result:
- snprintf calls > 0
- Log messages properly formatted
- "detect_error" appears in messages
- Render fails with parse error

## Why the Minimal Reproduction Plan Changed

Originally we thought we needed a minimal tree-sitter grammar. But now we understand:

1. The issue is NOT in the grammar
2. The issue is NOT in the parser logic
3. The issue IS in the C stdlib shim layer

The fix is simple: remove the macro, add a function declaration, let the linker resolve to c_shim.rs.

## Remaining Question

If fixing snprintf doesn't fix `has_error()`, we have a deeper issue. But my hypothesis is:

1. tree-sitter's `to_sexp()` uses snprintf → currently broken
2. tree-sitter's logging uses snprintf → currently broken
3. tree-sitter's actual parsing does NOT use snprintf → should work

The `has_error()` function just walks the tree looking for ERROR nodes. It shouldn't depend on snprintf. If fixing snprintf doesn't make log messages work but `has_error()` still returns false, THEN we need the minimal reproduction to investigate further.

## Test Commands

```bash
# Native test (baseline - should show has_error: true)
cd crates/pampa && cargo test test_broken_link_tree_sexp -- --nocapture

# WASM test (after fix - should match native)
cd hub-client && node test-wasm.mjs
```

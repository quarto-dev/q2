# wasm-qmd-parser

WASM build of the `pampa` qmd parser for use in browser environments.

## Important: Excluded from Workspace

This crate is **excluded from the default workspace build** because it cannot be compiled as a native shared library. The dependency chain pulls in V8 (via deno_core) which uses thread-local storage incompatible with native cdylib builds.

Build this crate explicitly using wasm-pack as described below.

## Build Instructions

```bash
cd crates/wasm-qmd-parser

# macOS only: Use Homebrew LLVM (Apple Clang doesn't support wasm32-unknown-unknown)
# Requires: brew install llvm
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"

# Set C flags for tree-sitter WASM compilation
# - Include our C shims from wasm-sysroot
# - Define HAVE_ENDIAN_H for tree-sitter's endian detection
export CFLAGS_wasm32_unknown_unknown="-I$(pwd)/wasm-sysroot -Wbad-function-cast -Wcast-function-type -fno-builtin -DHAVE_ENDIAN_H"

# Build with wasm-pack
# Note: Requires opt-level = "s" in workspace profile.dev to avoid "too many locals" error
wasm-pack build --target web --dev
```

## Output

The built package is output to `pkg/` and can be used directly in web applications or published to npm.

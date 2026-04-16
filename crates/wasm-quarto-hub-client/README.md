# wasm-quarto-hub-client

WASM client for the quarto-hub web frontend. Provides VFS (Virtual File System) management and document rendering capabilities for browser environments.

## Build Requirements

This crate targets `wasm32-unknown-unknown` and requires:

1. **Rust target**: `rustup target add wasm32-unknown-unknown`

2. **LLVM (macOS)**: `brew install llvm`

## Building

Build with the appropriate CFLAGS to use the custom sysroot headers:

```bash
cd crates/wasm-quarto-hub-client
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
export CFLAGS_wasm32_unknown_unknown="-I$(pwd)/wasm-sysroot -Wbad-function-cast -Wcast-function-type -fno-builtin -DHAVE_ENDIAN_H"
cargo build -p wasm-quarto-hub-client --target wasm32-unknown-unknown --release
```

The WASM file will be output to `target/wasm32-unknown-unknown/release/wasm_quarto_hub_client.wasm`.

## API

### VFS Management

- `vfs_add_file(path, content)` - Add a text file to VFS
- `vfs_add_binary_file(path, content)` - Add a binary file to VFS
- `vfs_remove_file(path)` - Remove a file from VFS
- `vfs_list_files()` - List all files in VFS
- `vfs_clear()` - Clear all files from VFS
- `vfs_read_file(path)` - Read a file from VFS

### Rendering

- `render_qmd(path)` - Render a QMD file from VFS
- `render_qmd_content(content, template_bundle)` - Render QMD content directly
- `get_builtin_template(name)` - Get a built-in template bundle

All functions return JSON responses.

## Debugging

### Testing WASM from the Command Line

The fastest way to debug WASM behavior is to test directly from Node.js without a browser. This avoids the browser dev tools loop and lets you iterate quickly.

Use the test script in `hub-client/test-wasm.mjs`:

```bash
cd hub-client
node test-wasm.mjs
```

This script:
1. Loads the compiled WASM module directly
2. Calls `render_qmd_content()` with test input
3. Prints all debug information including parse tree state

To test different inputs, edit the `brokenContent` variable in the script.

### Debug Counters

The WASM module includes debug counters that are logged to the console:

- `snprintf calls` - Number of times our snprintf implementation was called
- `detect_error seen` - Whether tree-sitter detected a parse error
- `tree_has_error` - Whether the parse tree contains ERROR nodes
- `log_observer_has_error` - Whether the log observer detected errors

These are visible in both browser console and Node.js output.

### Architecture: C Standard Library Shims

When compiling to WASM, there's no libc. The tree-sitter C code needs standard library functions like `malloc`, `free`, `snprintf`, etc. We provide these in two places:

1. **`wasm-sysroot/`** - Stub C headers that declare the functions. These are included via `CFLAGS_wasm32_unknown_unknown` during compilation.

2. **`src/c_shim.rs`** - Rust implementations of the C functions, exported with `#[no_mangle] extern "C"`.

**Critical**: The headers in `wasm-sysroot/` must declare functions (not define them as macros) so the C code actually calls our Rust implementations. For example:

```c
// CORRECT - function declaration, links to c_shim.rs
int snprintf(char *str, unsigned long n, const char *format, ...);

// WRONG - macro replacement, c_shim.rs never called
#define snprintf(str, len, ...) 0
```

### Common Issues

**Problem**: Parse errors not detected in WASM but work in native CLI.

**Diagnosis**: Check the debug counters:
- If `snprintf calls: 0`, the C code isn't calling our implementation
- Check `wasm-sysroot/stdio.h` for macro definitions that bypass our functions

**Problem**: Garbage or empty log messages from tree-sitter.

**Diagnosis**: Same as above - snprintf is likely being bypassed.

**Problem**: Changes to wasm-sysroot headers don't take effect.

**Solution**: Force a full rebuild by touching the build.rs:
```bash
touch crates/tree-sitter-qmd/bindings/rust/build.rs
node hub-client/scripts/build-wasm.js
```

Or delete the target directory entirely:
```bash
rm -rf target
node hub-client/scripts/build-wasm.js
```

### Build Script

Always use the build script in `hub-client/scripts/build-wasm.js` rather than running `wasm-pack` directly. The script sets the required `CFLAGS_wasm32_unknown_unknown` environment variable to include the wasm-sysroot headers.

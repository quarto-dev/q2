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

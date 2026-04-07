# WASM Code Rules

## Never add `test` to wasm32 cfg guards

The cfg pattern `#[cfg(any(target_arch = "wasm32", test))]` is prohibited. It forces
native tests through the WASM-restricted Lua stdlib, which fails on Windows.

Correct pattern:
```rust
#[cfg(target_arch = "wasm32")]
// WASM-specific code (restricted Lua stdlib, synthetic io/os)

#[cfg(not(target_arch = "wasm32"))]
// Native code (full Lua stdlib via Lua::new())
```

## Verify WASM tests when editing WASM code

When modifying any of these files, update `crates/pampa/tests/wasm_lua.rs`:
- `crates/pampa/src/lua/filter.rs` (cfg(target_arch = "wasm32") blocks)
- `crates/pampa/src/lua/shortcode.rs` (cfg(target_arch = "wasm32") blocks)
- `crates/pampa/src/lua/io_wasm.rs`
- `crates/pampa/src/lua/os_wasm.rs`

WASM tests can't run locally on Windows — they run in Linux CI.
See `dev-docs/wasm.md` for the local run command (Linux/macOS).

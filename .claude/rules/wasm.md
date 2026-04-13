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

//! C stdlib shims for `wasm32-unknown-unknown`.
//!
//! On `wasm32-unknown-unknown` there is no libc. C libraries compiled for this
//! target (tree-sitter, Lua via lua-src) reference symbols like `malloc`,
//! `fprintf`, `snprintf`, etc. that must be provided by Rust `#[no_mangle]`
//! functions.
//!
//! This crate is a no-op on native targets. On wasm32, it exports the full set
//! of C stdlib shims needed by the project's C dependencies.
//!
//! Used by:
//! - `wasm-quarto-hub-client` (production WASM build)
//! - `pampa` dev-dependencies (WASM integration tests)
//!
//! # Edition note
//!
//! This crate uses edition 2021 (not the workspace default of 2024). Edition
//! 2024 requires explicit `unsafe {}` blocks inside `unsafe fn` bodies. Since
//! nearly every line in the shims dereferences raw pointers, this would add
//! `unsafe {}` wrappers to ~65 call sites with no safety benefit — the functions
//! are all `unsafe extern "C"` FFI entry points.

#![cfg_attr(target_arch = "wasm32", feature(c_variadic))]

#[cfg(target_arch = "wasm32")]
mod shim;

/// Marker payload for panics raised by `rust_lua_throw` (the WASM replacement
/// for Lua's `LUAI_THROW`). Each Lua error inside `lua_pcall` produces one
/// such panic, which `rust_lua_protected_call` catches microseconds later.
///
/// Hosts that install a custom panic hook (e.g. wasm-quarto-hub-client) can
/// downcast to this type to filter expected Lua control-flow panics out of
/// console.error logs without suppressing real Rust panics.
pub struct LuaThrow;

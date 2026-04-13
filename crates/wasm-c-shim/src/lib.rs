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

#![cfg_attr(target_arch = "wasm32", feature(c_variadic))]

#[cfg(target_arch = "wasm32")]
mod shim;

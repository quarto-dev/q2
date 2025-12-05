/*
 * lua/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Lua filter support for quarto-markdown-pandoc.
 *
 * This module provides Pandoc-compatible Lua filter execution using mlua.
 * Elements are exposed as userdata with named field access (Pandoc 2.17+ style).
 */

mod constructors;
mod diagnostics;
mod filter;
mod json;
mod list;
pub mod mediabag;
mod path;
mod readwrite;
pub mod runtime;
mod system;
mod text;
mod types;
mod utils;

pub use filter::{LuaFilterError, apply_lua_filters};
#[allow(unused_imports)]
pub use runtime::{LuaRuntime, NativeRuntime, RuntimeError, RuntimeResult};

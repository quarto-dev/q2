/*
 * lua/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Lua filter support for pampa.
 *
 * This module provides Pandoc-compatible Lua filter execution using mlua.
 * Elements are exposed as userdata with named field access (Pandoc 2.17+ style).
 */

mod constructors;
mod diagnostics;
mod dofile_wasm;
mod filter;
mod io_wasm;
mod json;
mod list;
pub mod mediabag;
mod os_wasm;
mod path;
mod quarto_api;
mod quarto_doc;
mod readwrite;
pub mod runtime;
pub mod shortcode;
mod system;
mod text;
mod types;
mod utils;

#[allow(unused_imports)]
pub use filter::FilterOutput;
#[allow(unused_imports)]
pub use filter::{LuaFilterError, apply_lua_filter, apply_lua_filters, create_filter_environment};
#[allow(unused_imports)]
pub use quarto_doc::{
    HtmlDependency, IncludeLocation, TextInclude, extract_html_dependencies, extract_text_includes,
};
#[cfg(not(target_arch = "wasm32"))]
#[allow(unused_imports)]
pub use runtime::NativeRuntime;
#[allow(unused_imports)]
pub use runtime::{RuntimeError, RuntimeResult, SystemRuntime};
#[allow(unused_imports)]
pub use shortcode::{
    LuaShortcodeEngine, LuaShortcodeError, LuaShortcodeResult, ShortcodeArgs, ShortcodeCallContext,
};

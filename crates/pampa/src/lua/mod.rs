/*
 * lua/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Lua filter support for pampa.
 *
 * This module provides Pandoc-compatible Lua filter execution using mlua.
 * Elements are exposed as userdata with named field access (Pandoc 2.17+ style).
 */

mod config_value;
mod constructors;
mod diagnostics;
mod dialect;
mod dofile_wasm;
mod filter;
mod host_context;
mod io_wasm;
mod json;
mod list;
// lmd cell execution (bd-lmd-exploration-jqv2nbl4).
pub mod lmd;
pub mod mediabag;
// quarto.metadata.get and the `meta` proxy (bd-lmd-exploration-jqv2nbl4 spike 6).
mod metadata;
mod nodes;
mod os_exit;
mod os_wasm;
mod pandoc_doc;
mod path;
mod process_view;
mod quarto_api;
pub mod quarto_doc;
pub(crate) mod readwrite;
pub mod runtime;
pub mod shortcode;
mod show;
mod system;
mod text;
mod types;
mod utils;
mod version;
mod walk;

#[allow(unused_imports)]
pub use crate::dialect::ModuleCompileError;
#[allow(unused_imports)]
pub use crate::lua_options::{ExtensionRoot, HostContext, HostProject, LuaOptions, ModuleRoots};
#[allow(unused_imports)]
pub use dialect::{DialectError, SourceDialect, register_source_dialect};
#[allow(unused_imports)]
pub use filter::FilterOutput;
#[allow(unused_imports)]
pub use filter::{
    LuaFilterError, apply_lua_filter, apply_lua_filter_with, apply_lua_filters,
    apply_lua_filters_with, create_filter_environment, create_filter_environment_with,
};
#[allow(unused_imports)]
pub use quarto_api::{register_quarto_include, set_module_roots};
#[allow(unused_imports)]
pub use quarto_doc::{
    HtmlDependency, IncludeLocation, TextInclude, extract_html_dependencies, extract_text_includes,
};
#[allow(unused_imports)]
pub use readwrite::{ReadSource, register_read_sources, take_read_sources};
#[cfg(not(target_arch = "wasm32"))]
#[allow(unused_imports)]
pub use runtime::NativeRuntime;
#[allow(unused_imports)]
pub use runtime::{RuntimeError, RuntimeResult, SystemRuntime};
#[allow(unused_imports)]
pub use shortcode::{
    LuaShortcodeEngine, LuaShortcodeError, LuaShortcodeResult, ShadowEvent, ShortcodeArgs,
    ShortcodeCallContext,
};

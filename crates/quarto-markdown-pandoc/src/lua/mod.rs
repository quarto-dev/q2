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
mod filter;
mod types;

pub use filter::apply_lua_filters;

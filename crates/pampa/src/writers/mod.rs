/*
 * mod.rs
 * Copyright (c) 2025 Posit, PBC
 */

#[cfg(feature = "terminal-support")]
pub mod ansi;
pub mod html;
pub(crate) mod html_source;
pub mod incremental;
pub mod json;
pub(crate) mod json_stream;
pub mod native;
pub mod plaintext;
pub mod qmd;

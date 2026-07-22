/*
 * extension/mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Quarto extension discovery, parsing, and metadata contribution.
 */

//! Quarto extension support.
//!
//! Extensions are discovered from `_extensions/` directories in the project
//! hierarchy and parsed from `_extension.yml` files. They can contribute
//! format-specific metadata, filters, shortcodes, and other resources.
//!
//! Built-in extensions (e.g. `quarto/lipsum`) are embedded in the binary
//! and discovered before user extensions, but user extensions with the
//! same name take priority (last-match-wins in `find_extension`).

pub mod discover;
pub mod read;
pub mod types;

// Native-only: builds an extension's TS engine bundle by shelling out to
// `deno bundle` (mirrors `engine::ts_process`'s native gate — unavailable
// on wasm32-unknown-unknown).
#[cfg(not(target_arch = "wasm32"))]
pub mod build;

pub use discover::{discover_extensions, find_extension};
pub use read::read_extension;
pub use types::{Contributes, Extension, ExtensionFilter, ExtensionId};

#[cfg(not(target_arch = "wasm32"))]
mod builtin {
    use include_dir::{Dir, include_dir};

    use crate::resources::ResourceBundle;

    /// Built-in extensions embedded at compile time from `resources/extensions/`.
    static BUILTIN_EXTENSIONS_DIR: Dir =
        include_dir!("$CARGO_MANIFEST_DIR/../../resources/extensions");

    /// Resource bundle for built-in extensions. Lazily extracted to a temp
    /// directory on first access via `.path()`.
    pub static BUILTIN_EXTENSIONS: ResourceBundle =
        ResourceBundle::new("builtin-extensions", &BUILTIN_EXTENSIONS_DIR);
}

#[cfg(not(target_arch = "wasm32"))]
pub use builtin::BUILTIN_EXTENSIONS;

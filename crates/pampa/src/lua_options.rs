/*
 * lua_options.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! What the host hands a Lua environment beyond the document: the source
//! dialects its `require` may compile ([`crate::dialect`]) and the module
//! roots it may search ([`ModuleRoots`]). One struct, so the next
//! host-provided setting is a field here rather than a parameter on every
//! `_with` entry point (filters, shortcode handlers, lmd cells).
//!
//! Feature-independent (like `dialect.rs`) so `unified_filter` can carry it
//! whether or not the `lua-filter` feature is on.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::dialect::SourceDialect;

/// What the host knows about the document and project a Lua script runs
/// for: everything Quarto 1 surfaced through `quarto.project`,
/// `quarto.doc.input_file` / `quarto.doc.output_file`, `PANDOC_STATE`,
/// and the environment `os.getenv` reads (GH #939, #940). All optional:
/// pampa's own tests and the Lua conformance suite run without a host,
/// and then these registrations leave the paths nil and the profile list
/// empty.
#[derive(Clone, Debug, Default)]
pub struct HostContext {
    /// Absolute path of the input being rendered → `quarto.doc.input_file`.
    pub input_file: Option<PathBuf>,
    /// Absolute path of the output → `quarto.doc.output_file`.
    pub output_file: Option<PathBuf>,
    /// `None` when there is no host (the paths are then nil). A host
    /// rendering a single file passes the document's directory as the
    /// project, as Q1 did (GH #1013).
    pub project: Option<HostProject>,
    /// Active profile names → `quarto.project.profile` (a `pandoc.List`).
    pub profiles: Vec<String>,
    /// The environment `os.getenv` answers from: a snapshot the host took
    /// on its own thread before any script ran (the real environment,
    /// then file-defined `_environment` values for keys the real one
    /// lacks, then `QUARTO_PROFILE` forced to the active list). `None`
    /// means "no host": `os.getenv` is left to the stdlib on native and
    /// to the runtime on WASM. A snapshot keeps the stdlib's libc
    /// `getenv` off filter worker threads, where it would race any
    /// `setenv` elsewhere in the process.
    pub env: Option<Arc<HashMap<String, String>>>,
}

/// The project a document renders inside, as `quarto.project` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostProject {
    /// Project root, absolute.
    pub directory: PathBuf,
    /// Resolved output directory, absolute (the project root itself when
    /// the project declares no `output-dir`).
    pub output_directory: PathBuf,
    /// Relative path from the document's directory back to the project
    /// root, forward slashes, `"."` when they coincide — Q1's
    /// `projectOffset`.
    pub offset: String,
}

/// A directory `require("<name>/module")` and `quarto.include` may resolve
/// into by name: an extension's directory, addressed by the extension's
/// name (bd-76ogq4ew, phase 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionRoot {
    /// The name the first path segment of a module name is matched against
    /// (`cards` for `_extensions/cards/`).
    pub name: String,
    /// The extension's directory, absolute.
    pub dir: PathBuf,
}

/// Where the scoped `require` (and `quarto.include`) look after the
/// requiring file's own directory and the running script's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleRoots {
    /// The project root: `require("/components/cards")` resolves against
    /// it (the leading-`/` convention `components.src` uses). For a
    /// single-file render the host passes the document's directory.
    pub project: Option<PathBuf>,
    /// Extension directories, by extension name.
    pub extensions: Vec<ExtensionRoot>,
}

impl ModuleRoots {
    /// The extension root named `name`, if any. Later entries win, matching
    /// extension discovery's "user extensions override built-ins" order.
    pub fn extension(&self, name: &str) -> Option<&ExtensionRoot> {
        self.extensions.iter().rev().find(|e| e.name == name)
    }
}

/// Host-provided settings for a Lua environment.
#[derive(Clone, Default)]
pub struct LuaOptions {
    /// Source dialects `require` compiles to Lua, by file extension.
    pub dialects: Vec<Arc<dyn SourceDialect>>,
    /// Module roots beyond the script directories.
    pub module_roots: ModuleRoots,
    /// The `typst` binary the host found, served to Lua as
    /// `quarto.paths.typst()` (Q1 parity; the `placeholder` shortcode
    /// rasterizes with it, bd-gebcr1sa). `None` when there is none, which
    /// is always the case in WASM.
    pub typst: Option<PathBuf>,
    /// The document, project, and environment the scripts run for.
    pub host: HostContext,
}

impl LuaOptions {
    /// Dialects only, no extra roots.
    pub fn with_dialects(dialects: Vec<Arc<dyn SourceDialect>>) -> Self {
        Self {
            dialects,
            module_roots: ModuleRoots::default(),
            typst: None,
            host: HostContext::default(),
        }
    }
}

impl std::fmt::Debug for LuaOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LuaOptions")
            .field(
                "dialects",
                &self
                    .dialects
                    .iter()
                    .map(|d| d.name().to_string())
                    .collect::<Vec<_>>(),
            )
            .field("module_roots", &self.module_roots)
            .field("typst", &self.typst)
            .field("host", &self.host)
            .finish()
    }
}

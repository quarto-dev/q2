//! The render flags of a live preview session: `--to`, `-M` and
//! `--profile` (F8).
//!
//! The server holds them once, from [`crate::PreviewConfig::render`], and
//! uses them three ways:
//!
//! - `to` and `metadata` travel to the SPA in the `render` block of
//!   `GET /api/preview/config`; the browser applies them to every render
//!   it does (the html preview and the pandoc requests). The native routes
//!   take per-request `metadata` from the SPA, so they need no copy here.
//! - `metadata` also reaches **every engine run** of the server (the startup
//!   captures, a file change's re-execute, the Re-execute button and `auto`),
//!   as the runtime layer of [`runtime_with_overlay`], so `-M
//!   'execute: {record: true}'` makes the engine stage write the `.quarto`
//!   copy and `-M 'execute: {echo: false}'` changes what the code produces.
//!   The run is then made to describe the *source* again
//!   ([`quarto_core::record::overlay_free_run`]), because staleness is
//!   recomputed overlay-free; the source side (project discovery, inputs,
//!   staleness) uses the plain [`native_runtime`].
//! - `profile` selects the project profile for every **native** site
//!   through one runtime factory, [`native_runtime`]. The browser's runtime
//!   has no environment, so the html preview cannot honor it.
//!
//! Process-wide, like the other session state in this crate (one preview
//! server per process; nextest isolates tests). The factory itself is
//! instance-scoped: [`native_runtime_for`] builds a runtime for any
//! selection without touching the process environment.

use std::sync::OnceLock;

use quarto_core::project::project_profile::native_runtime_for_selection;
use quarto_system_runtime::NativeRuntime;

/// `--to`, `-M` and `--profile` as the server holds them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderFlags {
    /// The requested format (`--to`), as given.
    pub to: Option<String>,
    /// The `-M` overlay as one JSON object (the parse of
    /// `quarto_core::metadata_flag::parse_metadata_flags`); `None` when no
    /// `-M` was given.
    pub metadata: Option<serde_json::Value>,
    /// The `--profile` selection, as clap collected it (each value may
    /// itself be comma-separated). Empty when the flag was not given.
    pub profile: Vec<String>,
}

impl RenderFlags {
    /// The `render` block of `/api/preview/config`: `{ to?, metadata? }`,
    /// or `None` when neither flag was given (a flagless session, and a
    /// server older than the flags, answer without the block).
    pub fn config_block(&self) -> Option<serde_json::Value> {
        let mut block = serde_json::Map::new();
        if let Some(to) = &self.to {
            block.insert("to".to_string(), serde_json::Value::String(to.clone()));
        }
        if let Some(metadata) = &self.metadata {
            block.insert("metadata".to_string(), metadata.clone());
        }
        (!block.is_empty()).then_some(serde_json::Value::Object(block))
    }
}

static RENDER_FLAGS: OnceLock<RenderFlags> = OnceLock::new();

/// Stash the session's flags before the server starts serving.
/// First writer wins, like the other session statics.
pub(crate) fn set(flags: RenderFlags) {
    let _ = RENDER_FLAGS.set(flags);
}

/// The session's flags (all empty before [`set`]).
pub(crate) fn get() -> RenderFlags {
    RENDER_FLAGS.get().cloned().unwrap_or_default()
}

/// A native runtime for `profile` (the `--profile` selection). The
/// factory every native site of the preview server goes through; see
/// `quarto_core::project::project_profile::native_runtime_for_selection`.
pub fn native_runtime_for(profile: &[String]) -> NativeRuntime {
    native_runtime_for_selection(profile)
}

/// The session's native runtime with the `-M` overlay applied: what an **engine run** gets
/// (a lint enforces it), and what reads a metadata key before any pipeline runs
/// (`execute: record-sources`). The overlay is the runtime layer, the highest precedence.
pub(crate) fn runtime_with_overlay() -> std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> {
    let native: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
        std::sync::Arc::new(native_runtime());
    match get().metadata {
        Some(m) => std::sync::Arc::new(quarto_core::overlay_runtime::RenderOverlay::with_metadata(
            m,
        ))
        .apply(native),
        None => native,
    }
}

/// The session's native runtime: [`native_runtime_for`] the session's
/// `--profile`. Use this instead of `NativeRuntime::new()` anywhere the
/// server reads a project (a lint test enforces it).
pub fn native_runtime() -> NativeRuntime {
    native_runtime_for(RENDER_FLAGS.get().map_or(&[][..], |flags| &flags.profile))
}

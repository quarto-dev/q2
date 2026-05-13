//! `q2 preview` — Q2 replacement for the TypeScript Quarto `quarto
//! preview` command.
//!
//! Phase A (bd-yxqt) lands the CLI shape as a stub. The real boot
//! sequence — ephemeral quarto-hub spinup, SPA embedding, browser
//! launch — is added in Phase A.5 (bd-mflk). This file is the seam
//! between clap and the eventual `quarto_preview::run(config)`.

use std::path::PathBuf;

use anyhow::Result;
use quarto_core::QuartoError;

/// Concrete shape passed through from clap.
///
/// Mirrors the Phase A flag set in `claude-notes/plans/2026-05-13-q2-
/// preview-phase-a.md` §A.1. When A.5 implements the real boot, this
/// struct gets mapped into a `quarto_preview::PreviewConfig` (which
/// owns the runtime-level concerns like the temp-dir lifetime, the
/// hub server's `HubConfig`, and the SPA-from-disk override).
#[allow(dead_code)] // fields consumed in A.5 (bd-mflk) when execute() is wired
pub struct PreviewArgs {
    /// Project root or single file to preview. Default: current dir.
    pub path: Option<PathBuf>,
    /// Port to listen on. Default: random free port.
    pub port: Option<u16>,
    /// Host to bind to. Default: 127.0.0.1 (loopback only —
    /// `--insecure-allow-network` would gate any non-loopback host;
    /// not yet wired).
    pub host: Option<String>,
    /// Skip the browser-open step.
    pub no_browser: bool,
    /// Override the ephemeral samod storage dir.
    pub data_dir: Option<PathBuf>,
    /// Override the embedded SPA bundle with a disk path. Mirrors
    /// `QUARTO_TRACE_VIEWER_DIR`.
    pub preview_dir: Option<PathBuf>,
    /// Run standalone (no local project mode).
    pub no_project: bool,
}

pub fn execute(_args: PreviewArgs) -> Result<()> {
    // A.5 (bd-mflk) replaces this with `quarto_preview::run(config)`
    // once the CLI plumbing and the new `crates/quarto-preview/` crate
    // are wired together (A.2 = bd-yxqt's other half).
    Err(QuartoError::NotImplemented("preview".to_string()).into())
}

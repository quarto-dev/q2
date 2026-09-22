//! `q2 preview --static` — the Quarto-1-style preview (bd-sl79jjiq).
//!
//! Render the document or project to disk through the same path `q2
//! render` uses, serve the output directory over a static HTTP server,
//! and (unless `--no-watch`) watch the sources, re-render what changed,
//! and push a reload to the browser. None of the hub / SPA / WASM
//! machinery behind the default `q2 preview` is involved.
//!
//! Plan: `claude-notes/plans/2026-09-22-q2-preview-static.md`.

use std::path::PathBuf;

use anyhow::Result;
use quarto_core::QuartoError;
use quarto_core::format::FormatIdentifier;

/// Concrete shape passed through from clap (`Commands::Preview` with
/// `--static`). The hub-mode flags never reach here: clap rejects them
/// next to `--static` via `conflicts_with_all`.
pub struct StaticArgs {
    /// Project root or single file to preview. Default: current dir.
    pub path: Option<PathBuf>,
    /// Port to listen on. Default: probe an OS-assigned free port.
    pub port: Option<u16>,
    /// Host to bind to. Default: 127.0.0.1 (loopback only).
    pub host: Option<String>,
    /// Skip the browser-open step.
    pub no_browser: bool,
    /// Open the preview in this browser instead of the system default.
    pub browser: Option<String>,
    /// Render once and serve; never watch or re-render.
    pub no_watch: bool,
    /// After a re-render, reload in place instead of navigating to the
    /// changed page.
    pub no_navigate: bool,
    /// Explicit render format (`--to`), as `q2 render --to`.
    pub to: Option<String>,
}

/// Formats a static server can usefully show. `docx`/`pptx`/`epub`/
/// `typst` render fine but produce nothing to browse, so they are
/// refused up front rather than after a render (plan § CLI).
fn check_servable_format(to: Option<&str>) -> Result<()> {
    let Some(to) = to else {
        // No `--to`: the format comes from the document's front matter
        // and is checked once the render target is resolved (Phase 3).
        return Ok(());
    };
    let format = super::render::resolve_format(to)?;
    if matches!(
        format.identifier,
        FormatIdentifier::Html | FormatIdentifier::Revealjs
    ) {
        return Ok(());
    }
    anyhow::bail!(
        "Format '{}' is not supported with --static (only html and revealjs can be served)",
        format.identifier
    );
}

pub fn execute(args: StaticArgs) -> Result<()> {
    check_servable_format(args.to.as_deref())?;
    // Phase 0 (plan): the clap surface exists; the render/serve/watch
    // loop lands in Phase 3.
    let _ = (
        args.path,
        args.port,
        args.host,
        args.no_browser,
        args.browser,
        args.no_watch,
        args.no_navigate,
    );
    Err(QuartoError::NotImplemented("preview --static".to_string()).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servable_formats_pass_the_gate() {
        assert!(check_servable_format(None).is_ok());
        assert!(check_servable_format(Some("html")).is_ok());
        assert!(check_servable_format(Some("revealjs")).is_ok());
    }

    #[test]
    fn unservable_formats_are_refused_by_name() {
        for to in ["typst", "docx", "pptx", "epub"] {
            let err = check_servable_format(Some(to))
                .expect_err("format should be refused")
                .to_string();
            assert!(
                err.contains("not supported with --static"),
                "{to}: unexpected message {err}"
            );
            assert!(
                err.contains(to),
                "{to}: message should name the format: {err}"
            );
        }
    }
}

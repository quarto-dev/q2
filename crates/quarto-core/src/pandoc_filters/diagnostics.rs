//! Classification of a completed `pandoc` subprocess's stderr into
//! diagnostics.
//!
//! Pure — no subprocess, no filesystem. [`super::super::stage::stages`]'s
//! `PandocWriteStage` (`crates/quarto-core/src/stage/stages/pandoc_write.rs`)
//! owns the unconditional-capture wiring and the temp-JSON retention
//! decision; this module owns only the string classification and the
//! `Q-20-3` error shape, so it stays unit-testable without a real pandoc.
//!
//! See `claude-notes/plans/2026-09-18-pandoc-hybrid-P4-implementation.md`
//! Task 10.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};

use crate::stage::PipelineError;

/// Matches an ANSI SGR escape sequence (`\x1b[<params>m`) — not a full
/// terminal-control parser, just enough to strip the color codes Q1's
/// `lunacolors.lua` wraps every `quarto.warn()`/`quarto.error()` call in
/// unconditionally (no TTY check).
static ANSI_SGR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("valid regex"));

/// Strips ANSI SGR escape sequences from a single line.
fn strip_ansi(line: &str) -> std::borrow::Cow<'_, str> {
    ANSI_SGR.replace_all(line, "")
}

/// Extracts warning-shaped lines from a completed pandoc invocation's
/// stderr, re-emitting each as a `Q-11-1` ("Lua Filter Diagnostic")
/// warning — the same code `quarto.warn()` itself emits
/// (`crates/pampa/src/lua/diagnostics.rs`), reused here because the shape
/// matches: both are free-text diagnostics surfaced from inside the Lua
/// filter chain.
///
/// Two independent producers emit warning-shaped lines, and both must be
/// recognized:
///
/// - **pandoc's own logger** prefixes with `[WARNING]` (e.g. "Could not
///   fetch resource ...").
/// - **Q1's vendored `quarto.warn()`** (`filters/common/log.lua:24`) writes
///   `lunacolors.yellow("WARNING (" .. caller_info .. ") " .. message ..
///   "\n")`, and `lunacolors.yellow` (`filters/common/lunacolors.lua`)
///   wraps unconditionally in ANSI SGR codes — so the raw line looks like
///   `\x1b[33mWARNING (file:line) message`, matching neither `[WARNING]`
///   nor bare `WARNING` before stripping. (The trailing `\x1b[39m` reset
///   lands on its own line, since the message's `"\n"` is embedded inside
///   the colored text, before the closing escape.)
///
/// Not all stderr is warning-shaped: with `QUARTO_FILTER_DEPENDENCY_FILE`
/// unset, a zero-exit render dumps ~35 KB of `init.lua` source with no
/// warning prefix (Task 2's T2.4 covers the production cause). Lines that
/// match neither shape are dropped, not surfaced.
pub fn classify_pandoc_stderr(stderr: &str) -> Vec<DiagnosticMessage> {
    stderr
        .lines()
        .filter_map(|line| {
            let stripped = strip_ansi(line);
            if stripped.starts_with("[WARNING]") || stripped.starts_with("WARNING (") {
                Some(
                    DiagnosticMessageBuilder::warning(stripped.into_owned())
                        .with_code("Q-11-1")
                        .build(),
                )
            } else {
                None
            }
        })
        .collect()
}

/// Builds the `Q-20-3` error for a nonzero pandoc exit.
///
/// `status_desc` and `stderr` are both folded into the diagnostic's
/// *title*, not its `problem` field: `PipelineError`'s `Display` only ever
/// surfaces `diagnostics[0].title` (`crates/quarto-core/src/stage/error.rs`),
/// so putting the verbatim stderr there is what makes it visible to a
/// caller that only calls `.to_string()` on the returned error — which is
/// exactly the acceptance criterion ("an error whose payload contains the
/// pandoc stderr verbatim").
pub fn nonzero_exit_error(
    stage_name: &str,
    status_desc: &str,
    stderr: &str,
    retained_json_path: &Path,
) -> PipelineError {
    let message = DiagnosticMessageBuilder::error(format!(
        "pandoc exited with {status_desc}; input JSON retained at {}:\n{stderr}",
        retained_json_path.display()
    ))
    .with_code("Q-20-3")
    .build();
    PipelineError::stage_error_with_diagnostics(stage_name, vec![message])
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_error_reporting::DiagnosticKind;

    /// T10.1 (pure half): a single `[WARNING]` line produces exactly one
    /// `Q-11-1` warning diagnostic, carrying the line verbatim.
    ///
    /// The `if !status.success()` guard this task removes lives in
    /// `pandoc_write.rs`'s `classify_pandoc_completion`, not here — this
    /// test binds the classifier itself; `pandoc_write.rs`'s own
    /// `test_warning_on_successful_render_is_surfaced` binds the guard.
    #[test]
    fn test_warning_line_becomes_q11_1_diagnostic() {
        let diags = classify_pandoc_stderr(
            "[WARNING] Could not fetch resource img.png: replacing image with description\n",
        );

        assert_eq!(diags.len(), 1, "expected exactly one diagnostic");
        assert_eq!(diags[0].kind, DiagnosticKind::Warning);
        assert_eq!(diags[0].code.as_deref(), Some("Q-11-1"));
        assert!(
            diags[0]
                .title
                .contains("Could not fetch resource img.png: replacing image with description"),
            "expected the diagnostic to carry the warning line verbatim, got: {}",
            diags[0].title
        );
    }

    /// Finding 1 (final review): Q1's real `quarto.warn()`
    /// (`filters/common/log.lua:24`) wraps its message in
    /// `lunacolors.yellow`, so the raw stderr line looks like
    /// `\x1b[33mWARNING (file:line) message`, not `[WARNING] message`. The
    /// pre-fix classifier (bare `line.starts_with("[WARNING]")`) dropped
    /// every real Q1 warning silently.
    ///
    /// Revert hunk: removing the `strip_ansi` call (matching directly
    /// against the raw line) makes this RED, since the leading
    /// `\x1b[33m` escape means the raw line starts with neither
    /// `[WARNING]` nor `WARNING (`.
    #[test]
    fn test_ansi_wrapped_q1_warning_becomes_q11_1_diagnostic() {
        let stderr = "\x1b[33mWARNING (crossref/refs.lua:128) Unable to resolve crossref @fig-nope\n\x1b[39m";
        let diags = classify_pandoc_stderr(stderr);

        assert_eq!(diags.len(), 1, "expected exactly one diagnostic");
        assert_eq!(diags[0].kind, DiagnosticKind::Warning);
        assert_eq!(diags[0].code.as_deref(), Some("Q-11-1"));
        assert!(
            diags[0]
                .title
                .contains("Unable to resolve crossref @fig-nope"),
            "expected the diagnostic to carry the warning message with ANSI codes \
             stripped, got: {}",
            diags[0].title
        );
        assert!(
            !diags[0].title.contains('\x1b'),
            "expected the diagnostic title to be free of raw ANSI escapes, got: {:?}",
            diags[0].title
        );
    }

    /// T10.6: empty stderr on a clean render produces zero diagnostics —
    /// no spurious noise.
    ///
    /// Revert hunk: a classifier that emits a diagnostic per line
    /// unconditionally (rather than filtering on the `[WARNING]` prefix)
    /// makes this RED, since an empty string still yields one empty-line
    /// "line" under some splitting strategies; more generally, feeding
    /// non-`[WARNING]` stderr through such a classifier would also
    /// incorrectly produce diagnostics.
    #[test]
    fn test_clean_stderr_produces_no_diagnostics() {
        let diags = classify_pandoc_stderr("");
        assert!(diags.is_empty(), "expected zero diagnostics, got {diags:?}");

        // Also: non-`[WARNING]` noise (the measured ~35KB init.lua dump
        // shape) must not be surfaced either.
        let diags = classify_pandoc_stderr("some/init/lua/source/dump/with/no/warning/marker\n");
        assert!(
            diags.is_empty(),
            "expected non-[WARNING] stderr to be dropped, got {diags:?}"
        );
    }

    /// T10.2: the nonzero-exit handler wraps every line of stderr
    /// verbatim into the returned error's displayed text — not merely a
    /// substring like "stack traceback", which would pass even for a
    /// truncated capture.
    ///
    /// Revert hunk: replacing the `.title` construction with a generic
    /// "pandoc failed" message (dropping `stderr` from the title) makes
    /// this RED, since `PipelineError`'s `Display` only ever surfaces
    /// `diagnostics[0].title`.
    #[test]
    fn test_nonzero_exit_wraps_stderr_verbatim() {
        let traceback = "lua: /share/filters/main.lua:735: attempt to index a nil value\n\
             stack traceback:\n\
             \t[C]: in function 'error'\n\
             \t/share/pandoc/datadir/init.lua:42: in function <init.lua:40>";

        let err = nonzero_exit_error(
            "pandoc-write",
            "exit status: 83",
            traceback,
            Path::new("/tmp/quarto-pandoc-input.json"),
        );

        let rendered = err.to_string();
        for line in traceback.lines() {
            assert!(
                rendered.contains(line),
                "expected rendered error to contain line {line:?} verbatim, got:\n{rendered}"
            );
        }
    }

    /// The `Q-20-3` error also names the retained temp JSON path, so a
    /// caller reading only the error text (not inspecting the filesystem)
    /// still knows where to look.
    #[test]
    fn test_nonzero_exit_error_names_retained_json_path() {
        let err = nonzero_exit_error(
            "pandoc-write",
            "exit status: 83",
            "boom",
            Path::new("/tmp/quarto-pandoc-input-abc123.json"),
        );
        assert!(
            err.to_string()
                .contains("/tmp/quarto-pandoc-input-abc123.json"),
            "expected the error to name the retained JSON path, got: {err}"
        );
    }
}

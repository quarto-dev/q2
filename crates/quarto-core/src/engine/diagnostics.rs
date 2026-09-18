/*
 * engine/diagnostics.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Coded diagnostics for execution-engine failures (the `engine` / `Q-18-*`
 * subsystem).
 */

//! Coded diagnostics for execution-engine failures.
//!
//! This module is the single seam between [`ExecutionError`] and the
//! `DiagnosticMessage`s a user sees. `EngineExecutionStage` calls
//! [`engine_error_diagnostic`] for every engine failure instead of
//! flattening the error to a string, so each variant can carry a `Q-18-*`
//! code, a problem statement, details and hints. Variants without a
//! dedicated mapping yet fall through to a plain error with the variant's
//! `Display` text (bd-yd94iyq9 fills those in).
//!
//! Engines that need to raise a *warning* without failing (knitr's
//! unreadable include file, `Q-18-2`) attach it to
//! `ExecuteResult::warnings`; the stage drains those into its diagnostics.

use std::path::Path;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};

use super::ExecutionError;

/// Where users report engine-integration bugs.
const ISSUES_URL: &str = "https://github.com/quarto-dev/q2/issues";

/// The diagnostic for an engine failure.
///
/// `MalformedResult` maps to `Q-18-1`. Every other variant currently maps
/// to an uncoded error whose title is the variant's `Display` text —
/// exactly what the stage produced before this seam existed — so adding
/// a mapping here is the whole job of giving a variant a code
/// (bd-yd94iyq9).
pub fn engine_error_diagnostic(err: &ExecutionError) -> DiagnosticMessage {
    match err {
        ExecutionError::MalformedResult {
            engine,
            field_path,
            detail,
            preserved,
        } => {
            let preserved_note = match preserved {
                Some(path) => format!(
                    "The engine's raw result was preserved at {}.",
                    path.display()
                ),
                None => "The engine's raw result could not be preserved on disk.".to_string(),
            };
            DiagnosticMessageBuilder::error("Engine Returned an Unreadable Result")
                .with_code("Q-18-1")
                .problem(format!(
                    "the `{engine}` engine ran, but the result it returned is not in the \
                     shape Quarto expects — at `{field_path}`: {detail}."
                ))
                .add_detail(preserved_note)
                .add_hint(format!(
                    "This is a bug in Quarto's integration with the `{engine}` engine, not a \
                     problem in your document. Please report it at {ISSUES_URL}, including \
                     this message and the preserved file."
                ))
                .build()
        }
        other => DiagnosticMessage::error(other.to_string()),
    }
}

/// `Q-18-2`: the engine named an include file (`slot` is the Pandoc-style
/// slot name, e.g. `include-in-header`) that Quarto could not read. The
/// render continues without it, so this is a warning; `--strict` promotes
/// it.
pub fn unreadable_include_diagnostic(
    engine: &str,
    slot: &str,
    path: &Path,
    cause: &std::io::Error,
) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Engine Include File Could Not Be Read")
        .with_code("Q-18-2")
        .problem(format!(
            "the `{engine}` engine asked for the contents of {} to be placed in \
             `{slot}`, but the file could not be read: {cause}.",
            path.display()
        ))
        .add_detail(
            "The page was rendered without it, so content the engine expected there \
             (typically an HTML dependency's scripts and stylesheets) is missing.",
        )
        .add_hint(format!(
            "Re-run the render; if it recurs, check that the path exists and is readable, \
             or report it at {ISSUES_URL}. Use `--strict` to make this stop the render."
        ))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ExecutionError;
    use quarto_error_reporting::DiagnosticKind;
    use std::path::PathBuf;

    // ── T6 (bd-gy2ozix3): the malformed-result diagnostic ──────────────

    fn malformed() -> ExecutionError {
        ExecutionError::MalformedResult {
            engine: "knitr".to_string(),
            field_path: "includes.include-in-header".to_string(),
            detail: "invalid type: sequence, expected path string".to_string(),
            preserved: Some(PathBuf::from("/tmp/quarto-pipeline_abc/file123")),
        }
    }

    #[test]
    fn malformed_result_is_a_coded_error() {
        let diag = engine_error_diagnostic(&malformed());
        assert_eq!(diag.code.as_deref(), Some("Q-18-1"));
        assert_eq!(diag.kind, DiagnosticKind::Error);
        assert!(
            diag.location.is_none(),
            "a protocol error has no source span"
        );
    }

    #[test]
    fn malformed_result_text_names_engine_field_file_and_report_url() {
        let text = engine_error_diagnostic(&malformed()).to_text(None);
        assert!(text.contains("Q-18-1"), "{text}");
        assert!(text.contains("knitr"), "names the engine: {text}");
        assert!(
            text.contains("includes.include-in-header"),
            "names the field: {text}"
        );
        assert!(
            text.contains("invalid type: sequence, expected path string"),
            "carries serde's message: {text}"
        );
        assert!(
            text.contains("/tmp/quarto-pipeline_abc/file123"),
            "names the preserved file: {text}"
        );
        assert!(
            text.contains("https://github.com/quarto-dev/q2/issues"),
            "tells the user where to report: {text}"
        );
        assert!(
            text.contains("not") && text.contains("document"),
            "says the document is not at fault: {text}"
        );
    }

    #[test]
    fn malformed_result_without_preserved_file_says_so() {
        let err = ExecutionError::MalformedResult {
            engine: "knitr".to_string(),
            field_path: ".".to_string(),
            detail: "expected value".to_string(),
            preserved: None,
        };
        let text = engine_error_diagnostic(&err).to_text(None);
        assert!(text.contains("could not be preserved"), "{text}");
    }

    /// Variants without a dedicated mapping keep today's behaviour: an
    /// uncoded error carrying the variant's `Display` text.
    #[test]
    fn unmapped_variants_fall_through_to_display_text() {
        let err = ExecutionError::runtime_not_found("knitr", "Rscript");
        let diag = engine_error_diagnostic(&err);
        assert_eq!(diag.code, None);
        assert_eq!(diag.kind, DiagnosticKind::Error);
        assert_eq!(diag.title, err.to_string());
    }

    // ── the unreadable-include warning (Q-18-2) ──────────────────────────

    #[test]
    fn unreadable_include_is_a_coded_warning_naming_slot_path_and_cause() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "No such file");
        let diag = unreadable_include_diagnostic(
            "knitr",
            "include-in-header",
            std::path::Path::new("/tmp/x/header.html"),
            &io,
        );
        assert_eq!(diag.code.as_deref(), Some("Q-18-2"));
        assert_eq!(diag.kind, DiagnosticKind::Warning);
        let text = diag.to_text(None);
        assert!(text.contains("knitr"), "{text}");
        assert!(text.contains("include-in-header"), "{text}");
        assert!(text.contains("/tmp/x/header.html"), "{text}");
        assert!(text.contains("No such file"), "{text}");
    }
}

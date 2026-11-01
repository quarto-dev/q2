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
/// `MalformedResult` maps to `Q-18-1`, `RuntimeNotFound` to `Q-18-3`,
/// `NoHandlerForLanguage` to `Q-18-5`. Every
/// other variant currently maps to an uncoded error whose title is the
/// variant's `Display` text — exactly what the stage produced before this
/// seam existed — so adding a mapping here is the whole job of giving a
/// variant a code (bd-yd94iyq9).
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
        ExecutionError::RuntimeNotFound { engine, runtime } => {
            DiagnosticMessageBuilder::error("Engine Runtime Not Found")
                .with_code("Q-18-3")
                .problem(format!(
                    "the `{engine}` engine needs `{runtime}`, but it could not be found on \
                     this machine."
                ))
                .add_detail(
                    "This is a missing local dependency, not a problem in the document — the \
                     same document renders fine where the runtime is installed.",
                )
                .add_hint(format!(
                    "Install `{runtime}` and make sure it is on PATH, then re-run the render."
                ))
                .build()
        }
        ExecutionError::NoHandlerForLanguage { engine, language } => {
            no_runnable_kernel_diagnostic(engine, std::slice::from_ref(language), true)
        }
        other => DiagnosticMessage::error(other.to_string()),
    }
}

/// `Q-18-5`: the engine owns these languages but no installed kernel runs
/// them. One diagnostic per document, listing every language. An error in a
/// multi-engine document (`as_error`), where an `engine:` list means the
/// author said who runs what; a warning otherwise, where the cells are left
/// as written and the render continues. Both forms share the code and text.
pub fn no_runnable_kernel_diagnostic(
    engine: &str,
    languages: &[String],
    as_error: bool,
) -> DiagnosticMessage {
    let list = languages
        .iter()
        .map(|l| format!("`{l}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let builder = if as_error {
        DiagnosticMessageBuilder::error("Owned Language Has No Runnable Kernel")
    } else {
        DiagnosticMessageBuilder::warning("Owned Language Has No Runnable Kernel")
    };
    let builder = builder.with_code("Q-18-5").problem(format!(
        "the `{engine}` engine runs {list} cells, but no installed Jupyter kernel declares \
         that language."
    ));
    let builder = if as_error {
        builder
    } else {
        builder.add_detail("The cells are left as written and were not run.")
    };
    builder
        .add_hint(
            "Install a Jupyter kernel for the language (`jupyter kernelspec list` shows what \
             is installed), name a kernel for the whole document with `jupyter: <kernel>`, \
             or give the cell `eval: false` if it is only meant to be shown.",
        )
        .build()
}

/// A fence language that Quarto 2 invented and is removing. Not coded: it
/// goes away with the alias table after one release.
pub fn deprecated_alias_diagnostic(alias: &str, canonical: &str) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Deprecated Language Alias")
        .problem(format!(
            "`{{{alias}}}` cells are accepted for now, but the alias will be removed."
        ))
        .add_hint(format!("Write `{{{canonical}}}` instead."))
        .build()
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

/// The code for every inline-expression diagnostic (EXR-3 Phase 1; `Q-18-5` to `Q-18-9`
/// are claimed by other plans that have not landed, so the next free number is not the
/// next unclaimed one).
pub const INLINE_SPAN_CODE: &str = "Q-18-10";

/// What went wrong with one inline expression (a brace-spelling span such as
/// `` `{python} f(x)` ``).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineSpanProblem {
    /// The kernel raised while evaluating the span. Fatal; `error: true` does not apply.
    Raised { ename: String, evalue: String },
    /// The kernel finished without a value: a statement such as `x = 1`. Fatal.
    NoValue,
    /// The kernel returned output, but nothing with a text mime type. A warning: the
    /// span source stays in the document and the render succeeds.
    NoUsableResult,
    /// The span sits in a front matter value and its result cannot be written back into
    /// the YAML string (a newline would fold to a space). Fatal; names the key.
    FrontMatterValue { key: String, reason: String },
}

/// The one place the wording for an inline-expression problem lives (EXR-3
/// implementer note): the answer to bd-30k2j00f would be a change here.
///
/// `language` and `expr` are the span's token and body; `location` is
/// `file:line:column` when it resolves. [`InlineSpanProblem::NoUsableResult`] is a
/// warning, the others are errors (the caller turns an error into a failed render).
pub fn inline_span_diagnostic(
    problem: &InlineSpanProblem,
    language: &str,
    expr: &str,
    location: Option<&str>,
) -> DiagnosticMessage {
    let at = location.map(|l| format!(" at {l}")).unwrap_or_default();
    let span = format!("`{{{language}}} {expr}`");
    let builder = match problem {
        InlineSpanProblem::Raised { ename, evalue } => {
            DiagnosticMessageBuilder::error("Inline Expression Failed")
                .with_code(INLINE_SPAN_CODE)
                .problem(format!("inline expression{at} raised {ename}: {evalue}"))
                .add_detail(format!("The expression was {span}."))
                .add_hint(
                    "Fix the expression, or move the computation into a code cell. \
                     `error: true` does not apply to inline expressions.",
                )
        }
        InlineSpanProblem::NoValue => DiagnosticMessageBuilder::error("Inline Expression Failed")
            .with_code(INLINE_SPAN_CODE)
            .problem(format!("inline expression{at} produced no value"))
            .add_detail(format!(
                "The expression was {span}. An inline expression must be a single \
                 expression, not a statement such as an assignment."
            ))
            .add_hint("Move the statement into a code cell and refer to its value in the span."),
        InlineSpanProblem::FrontMatterValue { key, reason } => {
            DiagnosticMessageBuilder::error("Inline Expression Failed")
                .with_code(INLINE_SPAN_CODE)
                .problem(format!(
                    "the inline expression in front matter key `{key}`{at} cannot be \
                     written back into the front matter: {reason}"
                ))
                .add_detail(format!("The expression was {span}."))
                .add_hint(
                    "Make the expression return a single line of text, or move the \
                     computation into the document body.",
                )
        }
        InlineSpanProblem::NoUsableResult => {
            DiagnosticMessageBuilder::warning("Inline Expression Left Unevaluated")
                .with_code(INLINE_SPAN_CODE)
                .problem(format!(
                    "inline expression{at} produced no text value, so it was left as written"
                ))
                .add_detail(format!(
                    "The expression was {span}. The kernel returned output, but none of it \
                     had a text representation Quarto can place in the page."
                ))
                .add_hint("Make the expression return a string, a number, or a Markdown value.")
        }
    };
    builder.build()
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
        let err = ExecutionError::not_available("knitr");
        let diag = engine_error_diagnostic(&err);
        assert_eq!(diag.code, None);
        assert_eq!(diag.kind, DiagnosticKind::Error);
        assert_eq!(diag.title, err.to_string());
    }

    // ── the runtime-not-found error (Q-18-3, Q9) ─────────────────────────

    #[test]
    fn runtime_not_found_is_a_coded_error_naming_engine_and_runtime() {
        let err = ExecutionError::runtime_not_found("julia", "julia");
        let diag = engine_error_diagnostic(&err);
        assert_eq!(diag.code.as_deref(), Some("Q-18-3"));
        assert_eq!(diag.kind, DiagnosticKind::Error);
        let text = diag.to_text(None);
        assert!(text.contains("julia"), "{text}");
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

    // ── inline-expression diagnostics (EXR-3, Q-18-10) ───────────────────

    #[test]
    fn inline_raised_is_a_coded_error_naming_span_and_location() {
        let diag = inline_span_diagnostic(
            &InlineSpanProblem::Raised {
                ename: "NameError".into(),
                evalue: "name 'x' is not defined".into(),
            },
            "python",
            "x + 1",
            Some("doc.qmd:12:34"),
        );
        assert_eq!(diag.code.as_deref(), Some(INLINE_SPAN_CODE));
        assert_eq!(diag.kind, DiagnosticKind::Error);
        let text = diag.to_text(None);
        assert!(
            text.contains(
                "inline expression at doc.qmd:12:34 raised NameError: name 'x' is not defined"
            ),
            "{text}"
        );
        assert!(text.contains("`{python} x + 1`"), "{text}");
    }

    #[test]
    fn inline_no_value_is_a_coded_error() {
        let diag = inline_span_diagnostic(
            &InlineSpanProblem::NoValue,
            "python",
            "x = 1",
            Some("d.qmd:3:1"),
        );
        assert_eq!(diag.code.as_deref(), Some(INLINE_SPAN_CODE));
        assert_eq!(diag.kind, DiagnosticKind::Error);
        let text = diag.to_text(None);
        assert!(
            text.contains("inline expression at d.qmd:3:1 produced no value"),
            "{text}"
        );
        assert!(text.contains("`{python} x = 1`"), "{text}");
    }

    #[test]
    fn inline_no_usable_result_is_a_coded_warning_that_does_not_fail_the_render() {
        let diag = inline_span_diagnostic(
            &InlineSpanProblem::NoUsableResult,
            "python",
            "img",
            Some("d.qmd:5:2"),
        );
        assert_eq!(diag.code.as_deref(), Some(INLINE_SPAN_CODE));
        assert_eq!(diag.kind, DiagnosticKind::Warning);
        let text = diag.to_text(None);
        assert!(text.contains("d.qmd:5:2"), "{text}");
        assert!(text.contains("left as written"), "{text}");
    }

    #[test]
    fn inline_front_matter_value_is_a_coded_error_naming_the_key() {
        let diag = inline_span_diagnostic(
            &InlineSpanProblem::FrontMatterValue {
                key: "title".into(),
                reason: "the value contains a newline".into(),
            },
            "python",
            "f()",
            Some("d.qmd:2:8"),
        );
        assert_eq!(diag.code.as_deref(), Some(INLINE_SPAN_CODE));
        assert_eq!(diag.kind, DiagnosticKind::Error);
        let text = diag.to_text(None);
        assert!(
            text.contains("front matter key `title` at d.qmd:2:8"),
            "{text}"
        );
        assert!(text.contains("the value contains a newline"), "{text}");
    }

    #[test]
    fn inline_diagnostics_omit_the_location_when_it_does_not_resolve() {
        let diag = inline_span_diagnostic(&InlineSpanProblem::NoValue, "r", "x <- 1", None);
        let text = diag.to_text(None);
        assert!(
            text.contains("inline expression produced no value"),
            "{text}"
        );
        assert!(!text.contains(" at "), "{text}");
    }
}

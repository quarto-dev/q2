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

use quarto_source_map::SourceInfo;

use super::ExecutionError;
use super::context::ExecutionContext;
use super::units::{InlineUnit, Unit};
use crate::presenter::cell::describe_location;
use crate::presenter::document::PresentNote;

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

/// Why an engine's returned notebook could not become an execution record (EXR-7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineRecordProblem {
    /// The notebook is not a readable nbformat-4 notebook.
    Malformed { detail: String },
    /// The notebook has a different number of annotated units than the engine owns.
    /// Both counts are over the engine's own units only.
    CountMismatch { expected: usize, found: usize },
    /// A cell names a unit id the engine does not own (or that the document lacks).
    UnknownUnitId { id: String },
    /// Two cells name the same unit id.
    DuplicateUnitId { id: String },
    /// An inline span the engine owns has no result (EXR-12). `location` is where it sits
    /// in the author's file (`file:line`).
    MissingSpan { location: String },
    /// The record builder rejected the result.
    Build { detail: String },
}

/// The diagnostic for a notebook an engine returned that cannot be turned into a record.
///
/// A warning, not an error: while the record is additive (before an engine flips to core
/// presentation) the render itself is unaffected and only the record is dropped. `--strict`
/// promotes it. Whether a problem may fail the render after an engine flips is decided in
/// that engine's flip task. Not coded yet: `Q-18-5` to `Q-18-9` are claimed elsewhere.
pub fn engine_record_diagnostic(engine: &str, problem: &EngineRecordProblem) -> DiagnosticMessage {
    let what = match problem {
        EngineRecordProblem::Malformed { detail } => {
            format!("the notebook it returned is not readable: {detail}")
        }
        EngineRecordProblem::CountMismatch { expected, found } => format!(
            "it returned {found} annotated units, but it owns {expected} units in this \
             document"
        ),
        EngineRecordProblem::UnknownUnitId { id } => format!(
            "the notebook names unit `{id}`, which is not one of the units it owns in this \
             document"
        ),
        EngineRecordProblem::DuplicateUnitId { id } => {
            format!("the notebook names unit `{id}` more than once")
        }
        EngineRecordProblem::MissingSpan { location } => {
            format!("it reported no result for the inline expression at {location}, which it owns")
        }
        EngineRecordProblem::Build { detail } => {
            format!("the record could not be built from its result: {detail}")
        }
    };
    DiagnosticMessageBuilder::warning("Engine Execution Record Dropped")
        .problem(format!("the `{engine}` engine ran, but {what}."))
        .add_detail("The render is unaffected; only this engine's execution record was left out.")
        .add_hint(format!(
            "This is a bug in the `{engine}` engine's integration, not a problem in your \
             document. Please report it at {ISSUES_URL}. Use `--strict` to make this stop \
             the render."
        ))
        .build()
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

/// `Q-18-12`: knitr\'s cache is off for the run that records the document. One per document,
/// naming the chunks that asked for it. Informational: the output is the same, only slower.
pub fn knitr_cache_disabled_diagnostic(labels: &[String]) -> DiagnosticMessage {
    let list = labels
        .iter()
        .map(|l| format!("`{l}`"))
        .collect::<Vec<_>>()
        .join(", ");
    DiagnosticMessageBuilder::info("knitr Cache Is Off While Recording")
        .with_code("Q-18-12")
        .problem(format!(
            "these chunks asked for knitr\'s cache, and it was turned off for them: {list}."
        ))
        .add_detail(
            "Recording a chunk\'s output needs the chunk to run, and a cache hit skips the run, \
             so the chunks run every time this document is executed.",
        )
        .build()
}

/// `Q-18-13`: the document replaced a knitr hook, so that chunk and every later one keep
/// knitr\'s finished markdown. One per document, naming the hook and the first chunk.
pub fn knitr_hook_override_diagnostic(hook: &str, label: &str) -> DiagnosticMessage {
    DiagnosticMessageBuilder::info("knitr Hook Replaced By The Document")
        .with_code("Q-18-13")
        .problem(format!(
            "the chunk `{label}` replaced knitr\'s `{hook}` hook (`knitr::knit_hooks$set`)."
        ))
        .add_detail(
            "Quarto cannot reproduce what a replaced hook does from the recorded pieces, so \
             this chunk and every chunk after it keep knitr\'s finished markdown.",
        )
        .build()
}

/// `Q-18-14`: chunks with `child=` keep knitr\'s finished markdown. One per document, naming
/// the chunks.
pub fn knitr_child_diagnostic(labels: &[String]) -> DiagnosticMessage {
    let list = labels
        .iter()
        .map(|l| format!("`{l}`"))
        .collect::<Vec<_>>()
        .join(", ");
    DiagnosticMessageBuilder::info("knitr Child Document Kept As knitr Wrote It")
        .with_code("Q-18-14")
        .problem(format!(
            "these chunks include a child document with `child=`: {list}."
        ))
        .add_detail(
            "The child document is run by knitr and its finished markdown is kept as one block, \
             so its chunks are not presented from recorded pieces.",
        )
        .build()
}

/// `Q-18-15`: the document uses an engine that builds no execution record, so the
/// writer stored it in the older capture format (RIU-1). `with_records` are the engines
/// whose records exist; `without_records` are the ones that build none. A warning, with
/// every output still shown.
pub fn engines_without_record_diagnostic(
    with_records: &[String],
    without_records: &[String],
) -> DiagnosticMessage {
    let quoted = |names: &[String]| names.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>();
    let without = quoted(without_records);
    let subject = match without.as_slice() {
        [one] => format!("The engine {one} does not"),
        [a, b] => format!("The engines {a} and {b} do not"),
        many => {
            let (last, rest) = many.split_last().expect("at least one engine");
            format!("The engines {}, and {last} do not", rest.join(", "))
        }
    };
    let mut b = DiagnosticMessageBuilder::warning("Execution record skipped")
        .with_code("Q-18-15")
        .problem(format!(
            "{subject} build an execution record, so this document is stored in the older \
             capture format. Every cell still ran and every output is shown."
        ));
    if !with_records.is_empty() {
        b = b.add_detail(format!(
            "Engines that build records: {}",
            with_records.join(", ")
        ));
    }
    b.add_detail(format!(
        "Engines that do not: {}",
        without_records.join(", ")
    ))
    .add_hint("Nothing needs to change; your output is intact.")
    .add_hint(
        "Features that read execution records, like reusing saved results or sharing them with \
         other editors, may not work with this document.",
    )
    .add_hint(
        "An engine author can add record support (see the execution records page in the docs).",
    )
    .build()
}

/// The warning a page shows when no `--records` source supplied a record for a document that
/// has code cells (RIU-4), built from the walk's outcomes: one detail line per source, so the
/// reader sees why each one was passed over. `None` (silence) for a prose-only document and
/// when a source did supply.
pub fn no_record_source_diagnostic(
    outcomes: &[crate::record::SourceOutcome],
    has_code_cells: bool,
) -> Option<DiagnosticMessage> {
    use crate::record::OutcomeKind;
    if !has_code_cells
        || outcomes.is_empty()
        || outcomes.iter().any(|o| o.kind == OutcomeKind::Supplied)
    {
        return None;
    }
    let mut b = DiagnosticMessageBuilder::warning("No execution record for this document").problem(
        "None of the record sources named by `--records` supplied one, so no outputs are shown.",
    );
    for o in outcomes {
        let what = match &o.kind {
            OutcomeKind::Supplied => unreachable!("checked above"),
            OutcomeKind::Nothing => "nothing for this document".to_string(),
            OutcomeKind::Refused(why) => format!("refused: {why}"),
            OutcomeKind::Unavailable(why) => format!("not available: {why}"),
            OutcomeKind::Failed(why) => format!("failed: {why}"),
        };
        b = b.add_detail(format!("{}: {what}", o.source.label()));
    }
    Some(
        b.add_hint("Add `engine` to `--records` (and leave `preview.engine` on) to run the code.")
            .build(),
    )
}

/// The one-line form of [`engines_without_record_diagnostic`] for `provide-hub`, which has
/// no page to attach a diagnostic to.
pub fn engines_without_record_line(doc: &str, without_records: &[String]) -> String {
    let (noun, verb) = if without_records.len() == 1 {
        ("engine", "builds")
    } else {
        ("engines", "build")
    };
    let names = without_records
        .iter()
        .map(|n| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "note: {doc}: {noun} {names} {verb} no execution record, so this document is stored in \
         the older capture format (Q-18-15)."
    )
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

/// The code for an image output the presenter could not use.
pub const IMAGE_UNUSABLE_CODE: &str = "Q-18-11";

/// `Q-18-11`: a cell's image output (`mime`) could not be written, so the figure is
/// missing from the page. The render continues, so this is a warning; `--strict`
/// promotes it. `location` is `file:line:column` of the cell when it resolves.
pub fn image_unusable_diagnostic(
    mime: &str,
    reason: &str,
    location: Option<&str>,
) -> DiagnosticMessage {
    let at = location.map(|l| format!(" at {l}")).unwrap_or_default();
    DiagnosticMessageBuilder::warning("Image Output Could Not Be Used")
        .with_code(IMAGE_UNUSABLE_CODE)
        .problem(format!(
            "the `{mime}` output of the code cell{at} could not be used: {reason}"
        ))
        .add_detail(
            "The figure is missing from the page; the output shows the text \
             representation the kernel sent, or a placeholder when it sent none.",
        )
        .add_hint(format!(
            "Re-run the render; if it recurs, the kernel may be sending a malformed image. \
             Report it at {ISSUES_URL} with the kernel's name. Use `--strict` to make this \
             stop the render."
        ))
        .build()
}

/// The diagnostic for a span problem, located at the span's expression.
pub(crate) fn span_diagnostic(
    problem: &InlineSpanProblem,
    span: &InlineUnit,
    ctx: &ExecutionContext,
) -> quarto_error_reporting::DiagnosticMessage {
    let expr_source = SourceInfo::substring(
        ctx.source_info.clone(),
        span.code_start,
        span.code_start + span.code.len(),
    );
    let location = describe_location(&expr_source, 0, ctx);
    inline_span_diagnostic(problem, &span.lang, &span.code, location.as_deref())
}

/// A presenter note as the engine's warning: a span whose value has no usable text
/// stays in the document (`Q-18-10`, as before the flip).
pub(crate) fn note_diagnostic(
    note: &PresentNote,
    units: &[Unit],
    ctx: &ExecutionContext,
) -> quarto_error_reporting::DiagnosticMessage {
    match note {
        PresentNote::InlineNoUsableValue { unit } => match units.get(*unit) {
            Some(Unit::Inline(span)) => {
                span_diagnostic(&InlineSpanProblem::NoUsableResult, span, ctx)
            }
            _ => inline_span_diagnostic(&InlineSpanProblem::NoUsableResult, "?", "?", None),
        },
        PresentNote::ImageUnusable { unit, mime, reason } => {
            let location = match units.get(*unit) {
                Some(Unit::Cell(block)) => describe_location(
                    &SourceInfo::substring(
                        ctx.source_info.clone(),
                        block.code_start,
                        block.code_start + block.code.len(),
                    ),
                    0,
                    ctx,
                ),
                _ => None,
            };
            image_unusable_diagnostic(mime, reason, location.as_deref())
        }
        // Jupyter records hold no fallback units; the note exists for the knitr and marimo
        // adapters, which report it themselves.
        PresentNote::MarkdownFormatMismatch { stored, wanted, .. } => {
            quarto_error_reporting::DiagnosticMessageBuilder::warning(
                "Stored Markdown For Another Format",
            )
            .problem(format!(
                "a stored markdown unit was produced for {} but the document renders to `{wanted}`.",
                stored
                    .as_deref()
                    .map_or("no recorded format".to_string(), |s| format!("`{s}`"))
            ))
            .build()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ExecutionError;
    use quarto_error_reporting::DiagnosticKind;
    use std::path::PathBuf;

    // ── RIU-4: the "no source supplies" diagnostic ──────────────────────

    fn outcome(
        source: crate::record::RecordSource,
        kind: crate::record::OutcomeKind,
    ) -> crate::record::SourceOutcome {
        crate::record::SourceOutcome { source, kind }
    }

    #[test]
    fn no_source_diagnostic_lists_each_source_and_names_the_gate_variable() {
        use crate::record::{OutcomeKind as K, RecordSource as S};
        let outcomes = vec![
            outcome(S::Cache, K::Nothing),
            outcome(
                S::Q2,
                K::Refused("set QUARTO_ENGINE_RECORD_WRITE=1 to publish".into()),
            ),
            outcome(
                S::Engine,
                K::Unavailable("`engine` is not in --records".into()),
            ),
        ];
        let text = no_record_source_diagnostic(&outcomes, true)
            .expect("a document with code and no record is reported")
            .to_text(None);
        assert!(text.contains("cache: nothing"), "{text}");
        assert!(text.contains("QUARTO_ENGINE_RECORD_WRITE"), "{text}");
        assert!(text.contains("engine: not available"), "{text}");
    }

    #[test]
    fn no_source_diagnostic_is_silent_for_prose_or_when_a_source_supplied() {
        use crate::record::{OutcomeKind as K, RecordSource as S};
        let none = vec![outcome(S::Cache, K::Nothing)];
        assert!(no_record_source_diagnostic(&none, false).is_none());
        let supplied = vec![outcome(S::Cache, K::Nothing), outcome(S::Q2, K::Supplied)];
        assert!(no_record_source_diagnostic(&supplied, true).is_none());
        assert!(no_record_source_diagnostic(&[], true).is_none());
    }

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

    #[test]
    fn image_unusable_is_a_coded_warning_that_names_the_cell_and_the_reason() {
        let d = image_unusable_diagnostic(
            "image/png",
            "the payload is not valid base64",
            Some("a.qmd:7:1"),
        );
        assert_eq!(d.code.as_deref(), Some("Q-18-11"));
        assert_eq!(d.kind, DiagnosticKind::Warning);
        let text = d.to_text(None);
        assert!(text.contains("a.qmd:7:1"), "{text}");
        assert!(text.contains("image/png"), "{text}");
        assert!(text.contains("not valid base64"), "{text}");
    }

    // ── RIU-1: Q-18-15, an engine that builds no record ───────────────

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn engines_without_record_is_a_coded_warning_with_both_lists_and_hints() {
        let d = engines_without_record_diagnostic(&names(&["jupyter"]), &names(&["echo"]));
        assert_eq!(d.code.as_deref(), Some("Q-18-15"));
        assert_eq!(d.kind, DiagnosticKind::Warning);
        assert_eq!(d.title, "Execution record skipped");
        let text = d.to_text(None);
        assert!(
            text.contains("The engine \"echo\" does not build an execution record"),
            "{text}"
        );
        assert!(
            text.contains("Engines that build records: jupyter"),
            "{text}"
        );
        assert!(text.contains("Engines that do not: echo"), "{text}");
        assert!(text.contains("Nothing needs to change"), "{text}");
        assert!(text.contains("may not work with this document"), "{text}");
        assert!(
            text.contains("An engine author can add record support"),
            "{text}"
        );
    }

    #[test]
    fn engines_without_record_pluralizes_the_problem_line() {
        let d = engines_without_record_diagnostic(&names(&["jupyter"]), &names(&["a", "b"]));
        let text = d.to_text(None);
        assert!(
            text.contains("The engines \"a\" and \"b\" do not build"),
            "{text}"
        );
    }

    #[test]
    fn engines_without_record_line_is_one_line() {
        let l = engines_without_record_line("tour.qmd", &names(&["echo"]));
        assert_eq!(
            l,
            "note: tour.qmd: engine \"echo\" builds no execution record, so this document is \
             stored in the older capture format (Q-18-15)."
        );
        assert!(!l.contains('\n'));
    }
}

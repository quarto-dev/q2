//! Document import (epic `2026-10-03-document-import-epic.md`): everything about reading a
//! docx/odt/rtf/epub/pptx into a qmd except running pandoc and handling image bytes (I9).
//!
//! A pure, deterministic function of the source bytes' hash, pandoc's JSON, the media
//! manifest and the options (I20): nothing in this module touches the VFS or the clock.

pub mod formats;
pub mod report;
pub mod request;
pub mod transforms;

use pampa::pandoc::Pandoc;
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::By;

/// Read pandoc's JSON the way the CLI's `--from json` does: the lenient reader (pandoc's JSON
/// carries no source info), then `transform_divs`. A read error is Q-24-12.
pub fn read_pandoc_json(json_text: &str) -> Result<Pandoc, DiagnosticMessage> {
    let (pandoc, _context) =
        pampa::readers::json::read_completing_source_info(&mut json_text.as_bytes(), By::unknown())
            .map_err(|e| {
                report::internal_error(&format!("pandoc's JSON could not be read: {e}"))
            })?;
    let mut collector = pampa::utils::diagnostic_collector::DiagnosticCollector::new();
    Ok(pampa::pandoc::treesitter_utils::postprocess::transform_divs(pandoc, &mut collector))
}
pub mod media;

use media::{MediaPlanEntry, parse_manifest, plan_media, rewrite_image_targets};
use transforms::TransformCounts;

/// What `finish_import` returns (interface 2). A fatal error (Q-24-12) is `success: false`
/// with no `qmd` and no `media_plan`.
#[derive(Debug, Default)]
pub struct FinishOutcome {
    pub success: bool,
    pub diagnostics: Vec<DiagnosticMessage>,
    pub qmd: Option<String>,
    pub media_plan: Option<Vec<MediaPlanEntry>>,
}

fn count_diagnostics(counts: &TransformCounts) -> Vec<DiagnosticMessage> {
    let mut out = Vec::new();
    if counts.block_comments > 0 {
        out.push(report::comments_attached_to_block(counts.block_comments));
    }
    if counts.paragraph_marks > 0 {
        out.push(report::paragraph_marks_dropped(counts.paragraph_marks));
    }
    if counts.list_styles > 0 {
        out.push(report::list_styles_lost(counts.list_styles));
    }
    if counts.unmatched_markers > 0 {
        out.push(report::unmatched_comment_markers(counts.unmatched_markers));
    }
    out
}

/// Turn pandoc's JSON into qmd: read it leniently, run the import transforms, plan the media
/// and rewrite the image links, write the qmd, and report. A pure function of its arguments
/// (I20): no clock, no filesystem, no randomness.
///
/// `format` is the pandoc reader name `prepare_import` returned (`docx`, `pptx`, …); only
/// pptx changes the output (I6), and `None` means "not pptx".
pub fn finish_import(
    json_text: &str,
    stderr: &str,
    target_qmd_path: &str,
    media_manifest_json: &str,
    format: Option<&str>,
) -> FinishOutcome {
    let mut diagnostics = report::warnings_from_stderr(stderr);
    let fatal = |mut diagnostics: Vec<DiagnosticMessage>, error: DiagnosticMessage| {
        diagnostics.push(error);
        FinishOutcome {
            success: false,
            diagnostics,
            qmd: None,
            media_plan: None,
        }
    };

    let manifest = match parse_manifest(media_manifest_json) {
        Ok(m) => m,
        Err(e) => return fatal(diagnostics, e),
    };
    let pandoc = match read_pandoc_json(json_text) {
        Ok(p) => p,
        Err(e) => return fatal(diagnostics, e),
    };
    let (mut pandoc, counts) = transforms::apply(pandoc, format.unwrap_or(""));
    diagnostics.extend(count_diagnostics(&counts));

    let plan = plan_media(&manifest, target_qmd_path);
    if let Err(e) = rewrite_image_targets(&mut pandoc, &plan) {
        return fatal(diagnostics, e);
    }
    diagnostics.extend(plan.diagnostics);

    let mut buf = Vec::new();
    if let Err(errors) = pampa::writers::qmd::write(&pandoc, &mut buf) {
        let detail = errors
            .iter()
            .map(|e| e.title.clone())
            .collect::<Vec<_>>()
            .join("; ");
        return fatal(
            diagnostics,
            report::internal_error(&format!("the qmd could not be written: {detail}")),
        );
    }
    let Ok(qmd) = String::from_utf8(buf) else {
        return fatal(
            diagnostics,
            report::internal_error("the qmd writer produced invalid UTF-8"),
        );
    };
    FinishOutcome {
        success: true,
        diagnostics,
        qmd: Some(qmd),
        media_plan: Some(plan.entries),
    }
}

/// Turn a failed pandoc run into Q-24 diagnostics (interface 2). `kind` is the host's
/// `RunFailureKind`; `status` is `None` when pandoc didn't exit (crash, timeout).
pub fn classify_import_failure(
    kind: &str,
    status: Option<i32>,
    stderr: &str,
) -> Vec<DiagnosticMessage> {
    let message = pandoc_message(stderr);
    let diagnostic = match kind {
        report::FAILURE_PANDOC_EXIT => {
            let problem = if message.is_empty() {
                match status {
                    Some(code) => format!("The converter exited with status {code}."),
                    None => "The converter failed.".to_string(),
                }
            } else {
                message
            };
            report::pandoc_read_failed(
                &problem,
                Some(
                    "Is the file damaged, password-protected, or a different type than its extension says?",
                ),
            )
        }
        report::FAILURE_NO_OUTPUT => report::pandoc_read_failed(
            "The converter finished without producing any output.",
            Some("Is the file empty or damaged?"),
        ),
        report::FAILURE_OOM | report::FAILURE_CRASH | report::FAILURE_TIMEOUT => {
            report::pandoc_resources_exhausted(kind, &message)
        }
        report::FAILURE_INVALID_REQUEST | report::FAILURE_SUPERSEDED => {
            report::internal_error(&format!("the import run failed ({kind}): {message}"))
        }
        other => report::internal_error(&format!(
            "the import run failed with an unexpected kind \"{other}\""
        )),
    };
    vec![diagnostic]
}

/// pandoc's own error text from stderr: ANSI codes stripped, blank lines dropped, `[ERROR]`
/// prefixes removed, lines joined with a space.
fn pandoc_message(stderr: &str) -> String {
    let stripped = crate::pandoc_filters::diagnostics::strip_ansi_codes(stderr);
    stripped
        .lines()
        .map(|l| l.trim().trim_start_matches("[ERROR]").trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

//! KNR-2 Phase 2E: which output targets knitr documents are captured structurally for.
//!
//! The decision is made in Rust, which knows the target (the stage passes it to the
//! engine); a `Fallback` route tells R to skip structured capture for the whole document, and
//! the chunk hook then writes finished-markdown fallback units (KNR-3's chunk-kinds table,
//! epic decision 13). The structured set is what the KNR-1 corpus covers: HTML (and its
//! revealjs and epub variants), Typst, docx and gfm. Adding a target is one entry in [`STRUCTURED_TARGETS`], plus corpus cases.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};

/// Format identifiers (extensions such as `+smart` ignored) presented from structured units.
const STRUCTURED_TARGETS: &[&str] = &[
    "html", "html4", "html5", "revealjs", "epub", "typst", "docx", "gfm",
];

/// The LaTeX family, routed to fallback because kable and `df_print` choose LaTeX output by
/// format and the LaTeX epic owns that presentation.
const LATEX_TARGETS: &[&str] = &["latex", "beamer", "pdf"];

/// Why a document's chunks are captured as finished markdown instead of structured units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// `latex`, `beamer` or `pdf`.
    Latex,
    /// A target the corpus does not cover yet (pptx, context, ...).
    UncoveredTarget,
}

/// How to capture a knitr document for a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRoute {
    Structured,
    Fallback(FallbackReason),
}

/// Route `format` (a format identifier such as `html`, `pdf` or `typst+smart`).
pub fn route_for_target(format: &str) -> TargetRoute {
    let base = format.split('+').next().unwrap_or(format);
    if STRUCTURED_TARGETS.contains(&base) {
        TargetRoute::Structured
    } else if LATEX_TARGETS.contains(&base) {
        TargetRoute::Fallback(FallbackReason::Latex)
    } else {
        TargetRoute::Fallback(FallbackReason::UncoveredTarget)
    }
}

/// The one-per-document diagnostic for a fallback route.
pub fn fallback_diagnostic(reason: FallbackReason, format: &str) -> DiagnosticMessage {
    let (title, detail) = match reason {
        FallbackReason::Latex => (
            "knitr output for LaTeX is not presented from the execution record",
            "LaTeX targets keep knitr's finished markdown for every chunk.",
        ),
        FallbackReason::UncoveredTarget => (
            "knitr output for this target is not presented from the execution record",
            "Targets outside HTML, revealjs, epub, Typst, docx and gfm keep knitr's finished markdown for every chunk.",
        ),
    };
    DiagnosticMessageBuilder::info(title)
        .problem(format!("the output format is `{format}`."))
        .add_detail(detail)
        .build()
}

/// What `KnitrEngine::execute` does about capture for one run.
#[derive(Debug)]
pub struct CapturePlan {
    /// Ask R for each chunk's pieces (`KnitrExecuteParams.capture`).
    pub capture: bool,
    /// Said once per document when the target routes to fallback and a record is wanted.
    pub diagnostic: Option<DiagnosticMessage>,
}

/// Decide capture for a run. Pieces are asked for only when a record is wanted, the document
/// has units, and the target is structured; a fallback target skips capture and carries the
/// one-per-document diagnostic. With no record wanted nothing is captured and nothing is said.
pub fn plan_capture(want_record: bool, has_units: bool, format: &str) -> CapturePlan {
    if !want_record || !has_units {
        return CapturePlan {
            capture: false,
            diagnostic: None,
        };
    }
    match route_for_target(format) {
        TargetRoute::Structured => CapturePlan {
            capture: true,
            diagnostic: None,
        },
        TargetRoute::Fallback(reason) => CapturePlan {
            capture: false,
            diagnostic: Some(fallback_diagnostic(reason, format)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covered_targets_are_structured() {
        for f in [
            "html",
            "html5",
            "typst",
            "docx",
            "gfm",
            "revealjs",
            "epub",
            "html+smart",
        ] {
            assert_eq!(route_for_target(f), TargetRoute::Structured, "{f}");
        }
    }

    #[test]
    fn the_latex_family_falls_back() {
        for f in ["latex", "beamer", "pdf", "pdf+smart"] {
            assert_eq!(
                route_for_target(f),
                TargetRoute::Fallback(FallbackReason::Latex),
                "{f}"
            );
        }
    }

    #[test]
    fn an_uncovered_target_falls_back_until_the_corpus_covers_it() {
        for f in ["pptx", "context", "unknown-format"] {
            assert_eq!(
                route_for_target(f),
                TargetRoute::Fallback(FallbackReason::UncoveredTarget),
                "{f}"
            );
        }
    }

    #[test]
    fn capture_needs_a_record_units_and_a_structured_target() {
        let plan = plan_capture(true, true, "html");
        assert!(plan.capture && plan.diagnostic.is_none());
        for (want, units) in [(false, true), (true, false), (false, false)] {
            let plan = plan_capture(want, units, "pdf");
            assert!(!plan.capture && plan.diagnostic.is_none(), "{want} {units}");
        }
        let plan = plan_capture(true, true, "pdf");
        assert!(!plan.capture && plan.diagnostic.is_some());
    }

    #[test]
    fn the_diagnostic_names_the_target_and_the_reason() {
        let text = fallback_diagnostic(FallbackReason::Latex, "pdf").to_text(None);
        assert!(text.contains("pdf") && text.contains("LaTeX"), "{text}");
        let text = fallback_diagnostic(FallbackReason::UncoveredTarget, "epub").to_text(None);
        assert!(text.contains("epub") && !text.contains("LaTeX"), "{text}");
    }
}

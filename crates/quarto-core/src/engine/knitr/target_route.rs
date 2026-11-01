//! KNR-2 Phase 2E: which output targets knitr documents are captured structurally for.
//!
//! The decision is made in Rust, which knows the target (the stage passes it to the
//! engine); a `Fallback` route tells R to skip structured capture for the whole document, and
//! the chunk hook then writes finished-markdown fallback units (KNR-3's chunk-kinds table,
//! epic decision 13). The structured set is what the KNR-1 corpus covers: HTML (and its
//! revealjs and epub variants), Typst, docx and gfm. Adding a target is one entry in [`STRUCTURED_TARGETS`], plus corpus cases.

// Staged API: its one consumer is KNR-3's capture call, which decides skip-versus-capture per
// document. `expect` (not `allow`) so the build fails, and this line gets deleted, once that
// call exists. Tests exercise everything, hence the `not(test)`.
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "consumed by KNR-3's capture call, not yet wired")
)]

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
    fn the_diagnostic_names_the_target_and_the_reason() {
        let text = fallback_diagnostic(FallbackReason::Latex, "pdf").to_text(None);
        assert!(text.contains("pdf") && text.contains("LaTeX"), "{text}");
        let text = fallback_diagnostic(FallbackReason::UncoveredTarget, "epub").to_text(None);
        assert!(text.contains("epub") && !text.contains("LaTeX"), "{text}");
    }
}

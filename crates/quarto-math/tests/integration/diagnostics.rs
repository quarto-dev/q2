//! Problems → `Q-22-*` diagnostics.
//!
//! Every `ProblemKind` owns one code that exists in the live catalog under
//! subsystem `math` with the same title, errors and warnings are split the
//! way the plan says, and a diagnostic's location is the problem's span
//! mapped through the math text's own `SourceInfo`.

use std::collections::BTreeSet;

use quarto_error_reporting::DiagnosticKind;
use quarto_math::diagnostics::{code_for, diagnostics, severity_for, title_for};
use quarto_math::normalize::{Mode, ProblemKind, normalize};
use quarto_math::spec::Spec;
use quarto_source_map::{FileId, SourceInfo};

const ALL_KINDS: [ProblemKind; 13] = [
    ProblemKind::UnknownCommand,
    ProblemKind::UnknownEnvironment,
    ProblemKind::EnvironmentMismatch,
    ProblemKind::ArityMismatch,
    ProblemKind::UnbalancedBrace,
    ProblemKind::UnbalancedDelimiter,
    ProblemKind::DoubleScript,
    ProblemKind::ExpansionLimit,
    ProblemKind::Unsupported,
    ProblemKind::RaggedRows,
    ProblemKind::IgnoredLimits,
    ProblemKind::Dropped,
    ProblemKind::Syntax,
];

#[test]
fn every_problem_kind_has_a_distinct_catalog_code_under_math() {
    let catalog = &*quarto_error_catalog::ERROR_CATALOG;
    let mut seen = BTreeSet::new();
    for kind in ALL_KINDS {
        let code = code_for(kind);
        assert!(code.starts_with("Q-22-"), "{kind:?}: {code}");
        assert!(seen.insert(code), "{kind:?}: code {code} reused");
        let info = catalog
            .get(code)
            .unwrap_or_else(|| panic!("{code} ({kind:?}) is not in the error catalog"));
        assert_eq!(info.subsystem, "math", "{code}");
        assert_eq!(
            info.title,
            title_for(kind),
            "{code}: title drifted from the catalog"
        );
        assert_eq!(
            info.docs_url.as_deref(),
            Some(format!("https://quarto.org/docs/errors/math/{code}").as_str()),
            "{code}"
        );
    }
}

#[test]
fn severities_split_errors_from_caveats() {
    for kind in ALL_KINDS {
        let expected = match kind {
            ProblemKind::Unsupported
            | ProblemKind::RaggedRows
            | ProblemKind::IgnoredLimits
            | ProblemKind::Dropped => DiagnosticKind::Warning,
            _ => DiagnosticKind::Error,
        };
        assert_eq!(severity_for(kind), expected, "{kind:?}");
    }
}

#[test]
fn diagnostics_carry_code_title_message_and_severity() {
    let text = r"x \foo y";
    let n = normalize(text, Mode::Inline, Spec::builtin());
    let source = SourceInfo::original(FileId(0), 0, text.len());
    let ds = diagnostics(&n, &source);
    assert_eq!(ds.len(), 1, "{ds:?}");
    let d = &ds[0];
    assert_eq!(d.code.as_deref(), Some("Q-22-1"));
    assert_eq!(d.title, "Unknown Math Command");
    assert_eq!(d.kind, DiagnosticKind::Error);
    assert!(!d.hints.is_empty(), "unknown commands get a hint");
}

#[test]
fn location_is_the_problem_span_inside_the_math_text_source() {
    // Pretend the math text sits at bytes 40..48 of a file: `$x \foo y$`
    // with the `$` at 39 and 48.
    let text = r"x \foo y";
    let n = normalize(text, Mode::Inline, Spec::builtin());
    let source = SourceInfo::original(FileId(7), 40, 40 + text.len());
    let ds = diagnostics(&n, &source);
    let loc = ds[0].location.as_ref().expect("located");
    // `\foo` is text[2..6], so file bytes 42..46.
    assert_eq!(loc.preimage_in(FileId(7)), Some(42..46));
}

#[test]
fn a_problem_without_a_span_points_at_the_whole_expression() {
    // The macro engine's arity error token has no source bytes of its own.
    let text = r"\newcommand{\pr}[2]{P(#1|#2)} \pr{x}";
    let n = normalize(text, Mode::Inline, Spec::builtin());
    let source = SourceInfo::original(FileId(1), 100, 100 + text.len());
    let ds = diagnostics(&n, &source);
    let arity = ds
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-22-4"))
        .unwrap_or_else(|| panic!("{ds:?}"));
    assert_eq!(
        arity
            .location
            .as_ref()
            .and_then(|l| l.preimage_in(FileId(1))),
        Some(100..100 + text.len())
    );
}

#[test]
fn warnings_come_from_caveat_problems() {
    let text = "\\begin{pmatrix} a & b & c \\\\ d \\end{pmatrix}";
    let n = normalize(text, Mode::Display, Spec::builtin());
    let source = SourceInfo::original(FileId(0), 0, text.len());
    let ds = diagnostics(&n, &source);
    assert!(
        ds.iter()
            .any(|d| d.code.as_deref() == Some("Q-22-10") && d.kind == DiagnosticKind::Warning),
        "{ds:?}"
    );
}

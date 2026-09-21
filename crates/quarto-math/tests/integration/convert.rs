//! The writer-facing seam: `convert(text, mode, target, text_source, spec)`.

use quarto_error_reporting::DiagnosticKind;
use quarto_math::convert::{Target, convert};
use quarto_math::normalize::Mode;
use quarto_math::spec::Spec;
use quarto_source_map::{FileId, SourceInfo};

fn source(text: &str) -> SourceInfo {
    SourceInfo::original(FileId(0), 0, text.len())
}

#[test]
fn clean_input_converts_for_both_targets_without_diagnostics() {
    let text = r"\frac{a}{b} + \alpha^2";
    let omml = convert(
        text,
        Mode::Inline,
        Target::Omml,
        &source(text),
        Spec::builtin(),
    );
    assert!(omml.succeeded(), "{:?}", omml.diagnostics);
    assert!(omml.diagnostics.is_empty());
    assert!(omml.output.as_deref().unwrap().starts_with("<m:oMath>"));
    let typ = convert(
        text,
        Mode::Display,
        Target::Typst,
        &source(text),
        Spec::builtin(),
    );
    assert_eq!(typ.output.as_deref(), Some("(a)/(b) + α^(2)"));
}

#[test]
fn errors_withhold_output_and_report() {
    let text = r"x \foo y";
    let c = convert(
        text,
        Mode::Inline,
        Target::Omml,
        &source(text),
        Spec::builtin(),
    );
    assert!(
        c.output.is_none(),
        "an unknown command must not produce markup"
    );
    assert_eq!(c.diagnostics.len(), 1);
    let d = &c.diagnostics[0];
    assert_eq!(d.code.as_deref(), Some("Q-22-1"));
    assert_eq!(d.kind, DiagnosticKind::Error);
    assert_eq!(
        d.location.as_ref().and_then(|l| l.preimage_in(FileId(0))),
        Some(2..6)
    );
}

#[test]
fn warnings_accompany_output() {
    let text = "\\begin{pmatrix} a & b & c \\\\ d \\end{pmatrix}";
    let c = convert(
        text,
        Mode::Display,
        Target::Typst,
        &source(text),
        Spec::builtin(),
    );
    assert!(c.succeeded(), "{:?}", c.diagnostics);
    assert!(
        c.diagnostics
            .iter()
            .all(|d| d.kind == DiagnosticKind::Warning),
        "{:?}",
        c.diagnostics
    );
    assert_eq!(c.diagnostics[0].code.as_deref(), Some("Q-22-10"));
}

#[test]
fn locations_follow_the_text_source_offset() {
    // The math text sits at file bytes 100.. ; the unknown command at
    // text bytes 4..8 must resolve to 104..108.
    let text = r"a + \bad b";
    let ts = SourceInfo::original(FileId(3), 100, 100 + text.len());
    let c = convert(text, Mode::Inline, Target::Typst, &ts, Spec::builtin());
    let loc = c.diagnostics[0].location.as_ref().unwrap();
    assert_eq!(loc.preimage_in(FileId(3)), Some(104..108));
}

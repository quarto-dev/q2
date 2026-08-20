/*
 * tests/integration/project_type_parsing.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `project.type` parsing diagnostics (bd-ad7i1pc6, Phase 1).
 */

//! `_quarto.yml` `project.type` parsing contract.
//!
//! Before bd-ad7i1pc6, an unrecognized `project.type` silently fell
//! back to the default project kind (`.ok().unwrap_or_default()` in
//! `ProjectContext::parse_config`) — a `type: posit-docs` website
//! rendered as a bare default project with no indication anything was
//! wrong. These tests pin the replacement contract:
//!
//! - Unknown `project.type` → hard **Q-5-17** error naming the type,
//!   with a source snippet anchored in `_quarto.yml`.
//! - Non-string `project.type` → same Q-5-17 error.
//! - Built-in names (any case) parse as before; absent type = default.
//! - `book` / `manuscript` parse but render with default-project
//!   behavior today — that gets a visible **Q-5-18** warning instead
//!   of silence.

use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::error::QuartoError;
use quarto_core::project::{ProjectContext, ProjectKind};
use quarto_error_reporting::DiagnosticKind;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Discover a project whose `_quarto.yml` is `yaml`, keeping the
/// backing tempdir alive alongside the result.
fn discover(yaml: &str) -> (quarto_core::error::Result<ProjectContext>, TempDir) {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("_quarto.yml"), yaml).unwrap();
    std::fs::write(tmp.path().join("index.qmd"), "# Hello\n").unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = ProjectContext::discover(tmp.path(), runtime.as_ref());
    (result, tmp)
}

/// Unwrap the structured parse error from a failed discover.
fn parse_error(result: quarto_core::error::Result<ProjectContext>) -> quarto_core::ParseError {
    match result {
        Err(QuartoError::Parse(pe)) => pe,
        Err(other) => panic!("expected QuartoError::Parse, got: {other:?}"),
        Ok(_) => panic!("expected discovery to fail"),
    }
}

// ── Q-5-17: unknown / malformed `project.type` ──────────────────────

#[test]
fn unknown_project_type_is_a_q_5_17_error() {
    let (result, _tmp) = discover("project:\n  type: posit-docs\n");
    let pe = parse_error(result);

    assert_eq!(pe.diagnostics.len(), 1);
    let d = &pe.diagnostics[0];
    assert_eq!(d.kind, DiagnosticKind::Error);
    assert_eq!(d.code.as_deref(), Some("Q-5-17"));

    let text = pe.render();
    assert!(
        text.contains("posit-docs"),
        "error must name the unknown type; got: {text}"
    );
    // The hint lists the built-in type names so the user can spot typos.
    assert!(
        text.contains("website") && text.contains("manuscript"),
        "error must list built-in project types; got: {text}"
    );
    // The diagnostic is anchored at the `type:` scalar in _quarto.yml:
    // the source context must render a snippet naming the config file.
    assert!(
        text.contains("_quarto.yml"),
        "error must carry a source snippet from _quarto.yml; got: {text}"
    );
}

#[test]
fn non_string_project_type_is_a_q_5_17_error() {
    let (result, _tmp) = discover("project:\n  type: [website]\n");
    let pe = parse_error(result);

    assert_eq!(pe.diagnostics.len(), 1);
    assert_eq!(pe.diagnostics[0].code.as_deref(), Some("Q-5-17"));
    assert_eq!(pe.diagnostics[0].kind, DiagnosticKind::Error);
    let text = pe.render();
    assert!(
        text.contains("string"),
        "error must say the type must be a string; got: {text}"
    );
}

// ── Built-in names keep parsing ─────────────────────────────────────

#[test]
fn builtin_project_types_parse() {
    for (yaml, expected) in [
        ("project:\n  type: default\n", ProjectKind::Default),
        ("project:\n  type: website\n", ProjectKind::Website),
        ("project:\n  type: book\n", ProjectKind::Book),
        ("project:\n  type: manuscript\n", ProjectKind::Manuscript),
    ] {
        let (result, _tmp) = discover(yaml);
        let project = result.expect("built-in project type must parse");
        assert_eq!(project.project_kind(), expected, "yaml: {yaml}");
    }
}

#[test]
fn project_type_is_case_insensitive() {
    let (result, _tmp) = discover("project:\n  type: Website\n");
    let project = result.expect("case-insensitive type must parse");
    assert_eq!(project.project_kind(), ProjectKind::Website);
}

#[test]
fn absent_project_type_defaults_without_diagnostics() {
    let (result, _tmp) = discover("title: no project block\n");
    let project = result.expect("absent project.type must default");
    assert_eq!(project.project_kind(), ProjectKind::Default);
    assert!(
        quarto_core::project::project_kind_diagnostics(&project.config).is_empty(),
        "a default project must not warn"
    );
}

// ── Q-5-18: book / manuscript parse but warn ────────────────────────

#[test]
fn book_project_type_warns_q_5_18() {
    let (result, _tmp) = discover("project:\n  type: book\n");
    let project = result.expect("book must parse");
    assert_eq!(project.project_kind(), ProjectKind::Book);

    let diags = quarto_core::project::project_kind_diagnostics(&project.config);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code.as_deref(), Some("Q-5-18"));
    assert_eq!(diags[0].kind, DiagnosticKind::Warning);
    let text = diags[0].to_text(None);
    assert!(
        text.contains("book") && text.contains("default"),
        "warning must say book renders with default behavior; got: {text}"
    );
}

#[test]
fn manuscript_project_type_warns_q_5_18() {
    let (result, _tmp) = discover("project:\n  type: manuscript\n");
    let project = result.expect("manuscript must parse");
    assert_eq!(project.project_kind(), ProjectKind::Manuscript);

    let diags = quarto_core::project::project_kind_diagnostics(&project.config);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code.as_deref(), Some("Q-5-18"));
    assert!(diags[0].to_text(None).contains("manuscript"));
}

#[test]
fn website_project_type_does_not_warn() {
    let (result, _tmp) = discover("project:\n  type: website\n");
    let project = result.expect("website must parse");
    assert!(
        quarto_core::project::project_kind_diagnostics(&project.config).is_empty(),
        "website is fully implemented — no Q-5-18 warning"
    );
}

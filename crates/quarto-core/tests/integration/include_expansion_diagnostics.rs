/*
 * tests/integration/include_expansion_diagnostics.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Diagnostics contract for failing `{{< include >}}` shortcodes
 * (bd-qpvoamvu).
 */

//! Full-pipeline integration tests pinning what a failing
//! `{{< include file.qmd >}}` produces. Three contracts:
//!
//! 1. **An include that cannot supply its content fails the
//!    document.** A missing target, an unparseable target, or an
//!    include cycle leaves a hole where the included content should
//!    be, so the render aborts instead of writing a page with the
//!    hole in it (bd-include-parse-failure-dropped-u4rdjxru). This is
//!    the same outcome the identical parse error already gets when it
//!    is written inline on the page rather than one file away.
//!
//! 2. **Inner parse errors surface.** When the included file fails to
//!    parse, the Q-17-3 wrapper at the include site is followed by the
//!    included file's own parse diagnostics, whose locations resolve —
//!    through the error's `SourceContext` — to the *included* file,
//!    not the includer. (Previously the wrapper reported only an error
//!    count and the inner diagnostics were discarded.)
//!
//! 3. **No spurious "Unknown shortcode".** A failed include must not
//!    additionally produce Q-16-3 "Shortcode `include` is not
//!    recognized" from the downstream shortcode-resolve transform.
//!    These tests drive the full HTML pipeline — through
//!    `AstTransformsStage` — precisely so that regression is visible.
//!
//! Plans: `claude-notes/plans/2026-08-07-include-error-diagnostics.md`,
//! `claude-notes/plans/2026-09-02-failed-include-fails-document.md`.

use std::path::Path;
use std::sync::Arc;

use quarto_core::error::{ParseError, QuartoError};
use quarto_core::format::Format;
use quarto_core::pipeline::{HtmlRenderConfig, RenderOutput, render_qmd_to_html};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_error_reporting::DiagnosticMessage;

/// A fixture line that trips the qmd parser: the bare apostrophe after
/// a plural noun is read as a closing smart quote with no opener
/// (Q-2-10). Line 3 of `BAD_QMD` (0-indexed row 2).
const BAD_QMD: &str = "Some text before the error.\n\
    \n\
    This line mentions the groups' Unique IDs instead of their names.\n";

/// Write `files` into a temp dir and render `main` through the real
/// HTML pipeline, returning whatever the pipeline returned.
pub async fn try_render_fixture(
    files: &[(&str, &str)],
    main: &str,
) -> Result<RenderOutput, QuartoError> {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().to_path_buf();

    for (name, content) in files {
        std::fs::write(project_dir.join(name), content).unwrap();
    }

    let main_path = project_dir.join(main);
    let content = std::fs::read(&main_path).unwrap();

    let project = ProjectContext {
        dir: project_dir.clone(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&main_path)],
        output_dir: project_dir.clone(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&main_path);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
        Arc::new(quarto_system_runtime::NativeRuntime::new());

    render_qmd_to_html(
        &content,
        &main_path.to_string_lossy(),
        &mut ctx,
        &HtmlRenderConfig::default(),
        runtime,
    )
    .await
}

/// Render `main`, expecting it to succeed.
pub async fn render_fixture(files: &[(&str, &str)], main: &str) -> RenderOutput {
    match try_render_fixture(files, main).await {
        Ok(output) => output,
        Err(e) => panic!("expected the render to complete, got: {e}"),
    }
}

/// Render `main`, expecting the failure a broken include now produces,
/// and hand back the diagnostics it carries.
pub async fn render_fixture_err(files: &[(&str, &str)], main: &str) -> ParseError {
    match try_render_fixture(files, main).await {
        Err(QuartoError::Parse(parse_error)) => parse_error,
        Err(other) => panic!("expected a structured parse error, got: {other}"),
        Ok(_) => panic!("expected the render to fail, but it produced a document"),
    }
}

/// Resolve a diagnostic's primary location to `(file_name, row)`
/// against `source_context`.
fn resolved_location(
    d: &DiagnosticMessage,
    source_context: &quarto_source_map::SourceContext,
) -> Option<(String, usize)> {
    let loc = d.location.as_ref()?;
    let mapped = loc.map_offset(0, source_context)?;
    let file = source_context.get_file(mapped.file_id)?;
    let name = Path::new(&file.path)
        .file_name()?
        .to_string_lossy()
        .into_owned();
    Some((name, mapped.location.row))
}

pub fn codes(output: &RenderOutput) -> Vec<&str> {
    diagnostic_codes(&output.diagnostics)
}

pub fn error_codes(error: &ParseError) -> Vec<&str> {
    diagnostic_codes(&error.diagnostics)
}

fn diagnostic_codes(diagnostics: &[DiagnosticMessage]) -> Vec<&str> {
    diagnostics
        .iter()
        .filter_map(|d| d.code.as_deref())
        .collect()
}

fn assert_no_unknown_shortcode_in(diagnostics: &[DiagnosticMessage]) {
    for d in diagnostics {
        assert_ne!(
            d.code.as_deref(),
            Some("Q-16-3"),
            "failed include must not additionally report an unknown shortcode: {:?}",
            d
        );
        assert!(
            !d.title.contains("Unknown shortcode"),
            "failed include must not additionally report an unknown shortcode: {:?}",
            d
        );
    }
}

pub fn assert_no_unknown_shortcode_err(error: &ParseError) {
    assert_no_unknown_shortcode_in(&error.diagnostics);
}

#[tokio::test]
async fn parse_error_include_fails_the_document() {
    // The control this bug turns on: the identical parse error written
    // inline on the page already aborts the document. One file away it
    // must too — otherwise the page ships with a hole where the
    // included content should be.
    let error = render_fixture_err(
        &[
            (
                "index.qmd",
                "---\ntitle: Repro\n---\n\nBefore.\n\n{{< include \"_bad.qmd\" >}}\n\nAfter.\n",
            ),
            ("_bad.qmd", BAD_QMD),
        ],
        "index.qmd",
    )
    .await;

    assert!(
        error_codes(&error).contains(&"Q-17-3"),
        "parse failure must be reported: {:?}",
        error_codes(&error)
    );
}

#[tokio::test]
async fn parse_error_include_surfaces_inner_diagnostic() {
    let error = render_fixture_err(
        &[
            (
                "index.qmd",
                "---\ntitle: Repro\n---\n\nBefore.\n\n{{< include \"_bad.qmd\" >}}\n\nAfter.\n",
            ),
            ("_bad.qmd", BAD_QMD),
        ],
        "index.qmd",
    )
    .await;

    // The wrapper: anchored at the include site in index.qmd (line 7,
    // 0-indexed row 6).
    let wrapper = error
        .diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-17-3"))
        .unwrap_or_else(|| {
            panic!(
                "expected Q-17-3 wrapper, got codes {:?}",
                error_codes(&error)
            )
        });
    assert_eq!(
        resolved_location(wrapper, &error.source_context),
        Some(("index.qmd".to_string(), 6)),
        "wrapper must point at the include site"
    );

    // The inner diagnostic: the included file's own parse error, with
    // a location that resolves to _bad.qmd row 2 (the apostrophe line).
    let inner = error
        .diagnostics
        .iter()
        .find(|d| {
            resolved_location(d, &error.source_context).is_some_and(|(name, _)| name == "_bad.qmd")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected an inner diagnostic located in _bad.qmd; got: {:?}",
                error
                    .diagnostics
                    .iter()
                    .map(|d| (
                        d.code.clone(),
                        d.title.clone(),
                        resolved_location(d, &error.source_context)
                    ))
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        resolved_location(inner, &error.source_context).unwrap().1,
        2,
        "inner diagnostic must point at the offending line of _bad.qmd"
    );
    // The fixture's specific parse error today is the smart-quote
    // mismatch; pin the code so a silently different inner error
    // (which would mean the fixture no longer tests what it did)
    // fails loudly.
    assert_eq!(inner.code.as_deref(), Some("Q-2-10"));
}

#[tokio::test]
async fn parse_error_include_no_unknown_shortcode_warning() {
    let error = render_fixture_err(
        &[
            (
                "index.qmd",
                "---\ntitle: Repro\n---\n\n{{< include \"_bad.qmd\" >}}\n",
            ),
            ("_bad.qmd", BAD_QMD),
        ],
        "index.qmd",
    )
    .await;

    assert!(
        error_codes(&error).contains(&"Q-17-3"),
        "parse failure must be reported: {:?}",
        error_codes(&error)
    );
    assert_no_unknown_shortcode_err(&error);
}

#[tokio::test]
async fn missing_include_fails_the_document() {
    let error = render_fixture_err(
        &[(
            "index.qmd",
            "---\ntitle: Repro\n---\n\n{{< include \"_nonexistent.qmd\" >}}\n",
        )],
        "index.qmd",
    )
    .await;

    assert!(
        error_codes(&error).contains(&"Q-17-2"),
        "missing include must be reported: {:?}",
        error_codes(&error)
    );
    // Error severity, not warning: a page that quietly lost its main
    // content is not a warning-grade outcome, and an error is the one
    // severity `diagnostics:` suppression cannot silence.
    let missing = error
        .diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-17-2"))
        .unwrap();
    assert_eq!(missing.kind, quarto_error_reporting::DiagnosticKind::Error);
    assert_no_unknown_shortcode_err(&error);
}

#[tokio::test]
async fn circular_include_fails_the_document() {
    let error = render_fixture_err(
        &[
            (
                "index.qmd",
                "---\ntitle: Repro\n---\n\n{{< include \"_loop.qmd\" >}}\n",
            ),
            ("_loop.qmd", "Looping.\n\n{{< include \"index.qmd\" >}}\n"),
        ],
        "index.qmd",
    )
    .await;

    assert!(
        error_codes(&error).contains(&"Q-17-1"),
        "include cycle must be reported: {:?}",
        error_codes(&error)
    );
    let cycle = error
        .diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-17-1"))
        .unwrap();
    assert_eq!(cycle.kind, quarto_error_reporting::DiagnosticKind::Error);
    assert_no_unknown_shortcode_err(&error);
}

#[tokio::test]
async fn good_include_still_renders() {
    // The other side of the contract: an include that *can* supply its
    // content is untouched by any of this.
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "---\ntitle: Repro\n---\n\nBefore.\n\n{{< include \"_good.qmd\" >}}\n\nAfter.\n",
            ),
            ("_good.qmd", "Included content.\n"),
        ],
        "index.qmd",
    )
    .await;

    assert!(
        codes(&output).is_empty(),
        "a working include must render clean: {:?}",
        codes(&output)
    );
    assert!(
        output.html.contains("Included content."),
        "included content missing from the page:\n{}",
        output.html
    );
}

#[tokio::test]
async fn inline_include_reports_not_expanded_not_unknown() {
    // An include that is not the sole content of its paragraph is not
    // expanded by IncludeExpansionStage. It must NOT be reported as an
    // unknown shortcode ("check for typos" — the name is fine); it
    // gets the dedicated Q-17-4 "include not expanded here" warning.
    //
    // This one stays non-fatal: the shortcode is in a position the
    // expander does not serve, which is an authoring mistake about
    // placement rather than an include that tried and failed.
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "---\ntitle: Repro\n---\n\ntext {{< include \"_good.qmd\" >}} more\n",
            ),
            ("_good.qmd", "Good included content.\n"),
        ],
        "index.qmd",
    )
    .await;

    assert!(
        codes(&output).contains(&"Q-17-4"),
        "unexpanded include must get the dedicated diagnostic: {:?}",
        codes(&output)
    );
    assert_no_unknown_shortcode_in(&output.diagnostics);
}

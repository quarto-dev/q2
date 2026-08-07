/*
 * tests/integration/include_expansion_diagnostics.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Diagnostics contract for failing `{{< include >}}` shortcodes
 * (bd-qpvoamvu).
 */

//! Full-pipeline integration tests pinning the diagnostics a failing
//! `{{< include file.qmd >}}` produces. Two contracts:
//!
//! 1. **Inner parse errors surface.** When the included file fails to
//!    parse, the Q-17-3 wrapper at the include site is followed by the
//!    included file's own parse diagnostics, whose locations resolve —
//!    through the returned `SourceContext` — to the *included* file,
//!    not the includer. (Previously the wrapper reported only an error
//!    count and the inner diagnostics were discarded.)
//!
//! 2. **No spurious "Unknown shortcode".** A failed include (parse
//!    error, missing file, or include cycle) must not additionally
//!    produce Q-16-3 "Shortcode `include` is not recognized" from the
//!    downstream shortcode-resolve transform. These tests drive the
//!    full HTML pipeline — through `AstTransformsStage` — precisely so
//!    that regression is visible.
//!
//! Plan: `claude-notes/plans/2026-08-07-include-error-diagnostics.md`.

use std::path::Path;
use std::sync::Arc;

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
/// HTML pipeline, returning the collected diagnostics + source context.
async fn render_fixture(files: &[(&str, &str)], main: &str) -> RenderOutput {
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
    .expect("render completes (include failures are diagnostics, not fatal)")
}

/// Resolve a diagnostic's primary location to `(file_name, row)` via
/// the render output's `SourceContext`.
fn resolved_location(d: &DiagnosticMessage, output: &RenderOutput) -> Option<(String, usize)> {
    let loc = d.location.as_ref()?;
    let mapped = loc.map_offset(0, &output.source_context)?;
    let file = output.source_context.get_file(mapped.file_id)?;
    let name = Path::new(&file.path)
        .file_name()?
        .to_string_lossy()
        .into_owned();
    Some((name, mapped.location.row))
}

fn codes(output: &RenderOutput) -> Vec<&str> {
    output
        .diagnostics
        .iter()
        .filter_map(|d| d.code.as_deref())
        .collect()
}

fn assert_no_unknown_shortcode(output: &RenderOutput) {
    for d in &output.diagnostics {
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

#[tokio::test]
async fn parse_error_include_surfaces_inner_diagnostic() {
    let output = render_fixture(
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
    let wrapper = output
        .diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-17-3"))
        .unwrap_or_else(|| panic!("expected Q-17-3 wrapper, got codes {:?}", codes(&output)));
    assert_eq!(
        resolved_location(wrapper, &output),
        Some(("index.qmd".to_string(), 6)),
        "wrapper must point at the include site"
    );

    // The inner diagnostic: the included file's own parse error, with
    // a location that resolves to _bad.qmd row 2 (the apostrophe line).
    let inner = output
        .diagnostics
        .iter()
        .find(|d| resolved_location(d, &output).is_some_and(|(name, _)| name == "_bad.qmd"))
        .unwrap_or_else(|| {
            panic!(
                "expected an inner diagnostic located in _bad.qmd; got: {:?}",
                output
                    .diagnostics
                    .iter()
                    .map(|d| (
                        d.code.clone(),
                        d.title.clone(),
                        resolved_location(d, &output)
                    ))
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        resolved_location(inner, &output).unwrap().1,
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
    let output = render_fixture(
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
        codes(&output).contains(&"Q-17-3"),
        "parse failure must be reported: {:?}",
        codes(&output)
    );
    assert_no_unknown_shortcode(&output);
}

#[tokio::test]
async fn missing_include_reports_not_found_without_unknown_shortcode() {
    let output = render_fixture(
        &[(
            "index.qmd",
            "---\ntitle: Repro\n---\n\n{{< include \"_nonexistent.qmd\" >}}\n",
        )],
        "index.qmd",
    )
    .await;

    assert!(
        codes(&output).contains(&"Q-17-2"),
        "missing include must be reported: {:?}",
        codes(&output)
    );
    assert_no_unknown_shortcode(&output);
}

#[tokio::test]
async fn circular_include_reports_cycle_without_unknown_shortcode() {
    let output = render_fixture(
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
        codes(&output).contains(&"Q-17-1"),
        "include cycle must be reported: {:?}",
        codes(&output)
    );
    assert_no_unknown_shortcode(&output);
}

#[tokio::test]
async fn inline_include_reports_not_expanded_not_unknown() {
    // An include that is not the sole content of its paragraph is not
    // expanded by IncludeExpansionStage. It must NOT be reported as an
    // unknown shortcode ("check for typos" — the name is fine); it
    // gets the dedicated Q-17-4 "include not expanded here" warning.
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
    assert_no_unknown_shortcode(&output);
}

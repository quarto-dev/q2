/*
 * tests/integration/pandoc_typst_compile.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * pandoc-hybrid-typst Phase 2 — the compile step: `TypstCompileStage`
 * takes PandocWriteStage's intermediate `.typ` output and compiles it to
 * a real PDF via a real `typst` subprocess. See
 * claude-notes/plans/2026-09-18-pandoc-hybrid-typst.md, Phase 2.
 */

//! Unlike `pandoc_typst_writer.rs` (Phase 1, which deliberately overrides
//! the output path to a `.typ` file to sidestep the compile step and
//! inspect pandoc's raw text), these tests let `FormatIdentifier::Typst`'s
//! real `output_extension` (`"pdf"`) apply, so the full
//! `PandocWriteStage` -> `TypstCompileStage` chain runs end-to-end through
//! `render_document_to_file` and produces a real, inspectable PDF file —
//! not just an in-process assertion on AST/text shape.

use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// End-to-end: no `output_path` override, so `ctx.output_path()` is the
/// real `<stem>.pdf` — `TypstCompileStage` must actually run `typst
/// compile` against the vendored template (which unconditionally imports
/// the `marginalia` package) and produce a real PDF. Confirms the
/// unconditional package-cache staging (`extract_typst_packages`) is
/// sufficient for a hermetic compile — no network fetch.
#[test]
fn render_document_to_file_typst_compiles_to_real_pdf() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ntitle: Real PDF Compile\n---\n\n# Heading\n\nBody text.\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    let result = render_document_to_file(
        &input_path,
        "typst",
        &options,
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("typst render should succeed and produce a real PDF");

    let output_path = result.output_path;
    assert_eq!(
        output_path.extension().and_then(|e| e.to_str()),
        Some("pdf")
    );

    let bytes = std::fs::read(&output_path)
        .unwrap_or_else(|e| panic!("expected {} to exist: {e}", output_path.display()));
    assert!(
        bytes.starts_with(b"%PDF-"),
        "expected a real PDF header at {}, got: {:?}",
        output_path.display(),
        &bytes[..bytes.len().min(20)]
    );

    // Default `keep-typ` is false — the intermediate `.typ` file must not
    // survive a successful compile.
    let typ_path = output_path.with_extension("typ");
    assert!(
        !typ_path.exists(),
        "intermediate {} should have been removed (keep-typ defaults to false)",
        typ_path.display()
    );
}

/// `keep-typ: true` retains the intermediate `.typ` file alongside the
/// compiled PDF — the Q1-parity default-off knob (`core/typst.ts`'s
/// `kKeepTyp`), ported directly.
#[test]
fn render_document_to_file_typst_keep_typ_retains_intermediate() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ntitle: Keep Typ\nkeep-typ: true\n---\n\nBody text.\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    let result = render_document_to_file(
        &input_path,
        "typst",
        &options,
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("typst render should succeed");

    let output_path = result.output_path;
    let typ_path = output_path.with_extension("typ");
    assert!(
        typ_path.exists(),
        "expected the intermediate {} to survive with keep-typ: true",
        typ_path.display()
    );
    let typ_text = std::fs::read_to_string(&typ_path).unwrap();
    assert!(
        typ_text.contains("#show: doc => article("),
        "expected the retained .typ to be real pandoc typst source, got:\n{typ_text}"
    );
}

/// Regression test for a real bug this session found: `TypstCompileStage`
/// passed the wrong directory to `--package-cache-path` (missing a
/// `packages/` nesting level), so the vendored packages were never actually
/// found there — every prior Phase 2 test happened to render a document
/// that never triggers `definitions.typ`'s `$if(margin-geometry)$`-gated
/// `#import "@preview/marginalia:0.3.1"`, so the mismatch went unnoticed and
/// silently fell back to a live network fetch. A `.column-margin` div is
/// the simplest reachable way to set `margin-geometry` (via
/// `layout/meta.lua`'s `marginReferences()`/`hasMarginColumn`) without
/// depending on the still-open `quartoColumnParams` margin-notes wiring.
///
/// `HOME`/`XDG_CACHE_HOME` are isolated to a fresh temp dir (this
/// developer's machine has marginalia in its real `~/Library/Caches/typst`
/// from prior unrelated `typst` use, which would mask the bug), and
/// `HTTP(S)_PROXY` point at an unroutable address so any network fallback
/// fails fast and loud (`Connection refused`) instead of silently
/// succeeding via the network — deterministically distinguishing "found
/// hermetically" from "fell back to the network" without depending on
/// real network access in CI.
#[test]
fn render_document_to_file_typst_margin_note_compiles_without_network() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ntitle: Margin Note\n---\n\nHello world.\n\n::: {.column-margin}\nA margin note.\n:::\n",
    );

    let isolated_home = TempDir::new().unwrap();
    // SAFETY/scope: process-local under nextest (one process per test) —
    // see `fail_fast.rs`'s docstring for the isolation rationale.
    unsafe {
        std::env::set_var("HOME", isolated_home.path());
        std::env::set_var("XDG_CACHE_HOME", isolated_home.path().join(".cache"));
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");
        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:1");
    }

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    let result = render_document_to_file(
        &input_path,
        "typst",
        &options,
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect(
        "typst render with a margin note should compile hermetically from the vendored \
         package cache, with no network fallback",
    );

    let bytes = std::fs::read(&result.output_path).unwrap();
    assert!(
        bytes.starts_with(b"%PDF-"),
        "expected a real PDF header, got: {:?}",
        &bytes[..bytes.len().min(20)]
    );
}

/// A crossref'd figure + equation compiles all the way to a real PDF —
/// the actual end-to-end path Phase 1's `render_document_to_file_typst_
/// crossref_figure_and_equation_use_native_numbering` verified only at
/// the `.typ`-text level.
#[test]
fn render_document_to_file_typst_crossref_document_compiles_to_pdf() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ntitle: Crossref PDF\n---\n\nSee @fig-x.\n\n![Caption](image.svg){#fig-x}\n",
    );
    // A trivial, genuinely valid SVG — plain XML text, no checksums to
    // get wrong by hand (unlike PNG's CRC32-per-chunk framing).
    write(
        &project_dir.join("image.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\">\
         <rect width=\"10\" height=\"10\" fill=\"black\"/></svg>",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    let result = render_document_to_file(
        &input_path,
        "typst",
        &options,
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("typst render with a crossref'd figure should compile to a real PDF");

    let bytes = std::fs::read(&result.output_path).unwrap();
    assert!(
        bytes.starts_with(b"%PDF-"),
        "expected a real PDF header, got: {:?}",
        &bytes[..bytes.len().min(20)]
    );
}

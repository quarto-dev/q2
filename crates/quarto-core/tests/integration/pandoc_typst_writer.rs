/*
 * tests/integration/pandoc_typst_writer.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * pandoc-hybrid-typst Phase 1 — writer wiring, verified at the pipeline
 * level (not through the CLI's `render.rs` gate, which stays closed for
 * `Typst` until Phase 2's compile stage exists — see
 * claude-notes/plans/2026-09-18-pandoc-hybrid-typst.md).
 */

//! `render_document_to_file` already routes any non-native format through
//! `render_qmd_to_pandoc` (P7-foundation); these tests exercise that path
//! directly with `format = "typst"` and an explicit `.typ` output path
//! (`RenderToFileOptions.output_path`), sidestepping
//! `FormatIdentifier::Typst`'s real `output_extension` ("pdf" — correct for
//! the eventual compiled artifact, but not what `PandocWriteStage` should
//! hand to pandoc's `-t` flag in Phase 1).

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Pandoc's `-t` argument must be the writer name `"typst"`, not
/// `FormatIdentifier::Typst`'s final `output_extension` (`"pdf"`) — passing
/// `-t pdf` would ask pandoc for a from-scratch PDF pipeline (skipping the
/// typst writer, the vendored Lua filters, and any future `--template`
/// entirely). A real typst source file starts a level-1 heading with `=`
/// (Pandoc's typst writer emits ATX-style headings as `=`/`==`/...).
#[test]
fn render_document_to_file_typst_writes_real_typst_source() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ntitle: F\n---\n\n# Heading\n\nHello typst body.\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output_path = project_dir.join("f.typ");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };

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

    assert_eq!(result.output_path, output_path);
    let text = std::fs::read_to_string(&output_path).expect("output file should be readable");
    assert!(
        text.contains("Hello typst body."),
        "expected the body text to survive the typst writer, got:\n{text}"
    );
    assert!(
        !text.trim().is_empty(),
        "typst output must not be empty text"
    );
}

/// `format-typst.ts:92-100`'s `shift-heading-level-by: -1`, end to end
/// through the real pandoc binary: a document whose only heading is
/// level 2 (no level-1 heading anywhere) must come out shifted to `=`
/// (typst's level-1 marker), not `==`.
#[test]
fn render_document_to_file_typst_shifts_headings_when_no_level_one_present() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(&input_path, "## Sub Heading\n\nBody text.\n");

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output_path = project_dir.join("f.typ");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };

    render_document_to_file(
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

    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        text.lines().any(|l| l.trim() == "= Sub Heading"),
        "expected the level-2 heading shifted to typst's level-1 `=`, got:\n{text}"
    );
    assert!(
        !text.contains("== Sub Heading"),
        "heading must not stay at its original level, got:\n{text}"
    );
}

/// The other polarity: a document that already has a level-1 heading
/// must not be shifted at all.
///
/// Revert hunk: dropping the `has_level_one_heading` check in
/// `shift_heading_level_by_for` (always returning `Some(-1)`) makes this
/// RED — the level-1 heading would come out shifted to a nonsensical
/// level-0.
#[test]
fn render_document_to_file_typst_does_not_shift_when_level_one_present() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(&input_path, "# Top Heading\n\n## Sub Heading\n\nBody.\n");

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output_path = project_dir.join("f.typ");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };

    render_document_to_file(
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

    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        text.lines().any(|l| l.trim() == "= Top Heading"),
        "expected the top heading to stay at level 1, got:\n{text}"
    );
    assert!(
        text.lines().any(|l| l.trim() == "== Sub Heading"),
        "expected the sub heading to stay at level 2 (unshifted), got:\n{text}"
    );
}

/// `format-typst.ts:82-90`'s `section-numbering: "1.1.a"`, end to end:
/// `number-sections: true` in the document's own metadata must reach
/// pandoc's typst writer as a `section-numbering` metadata key — pandoc's
/// built-in default typst template (used here in the absence of Phase
/// 1's later template-vendoring work) forwards it verbatim into its
/// `sectionnumbering` template variable, confirmed empirically against
/// real pandoc 3.11 before writing this test.
///
/// Revert hunk: emptying `insert_typst_section_numbering`'s body makes
/// this RED (the key never reaches the metadata block pandoc serializes).
#[test]
fn render_document_to_file_typst_forwards_section_numbering_metadata() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ntitle: F\nnumber-sections: true\n---\n\n# Heading\n\nBody.\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output_path = project_dir.join("f.typ");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };

    render_document_to_file(
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

    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        text.contains("1.1.a"),
        "expected the section-numbering value to reach pandoc's typst output, got:\n{text}"
    );
}

/// Without `number-sections`, no `section-numbering` value should reach
/// the output at all — the other polarity of the discriminator above.
#[test]
fn render_document_to_file_typst_omits_section_numbering_by_default() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(&input_path, "---\ntitle: F\n---\n\n# Heading\n\nBody.\n");

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output_path = project_dir.join("f.typ");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };

    render_document_to_file(
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

    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        !text.contains("1.1.a"),
        "expected no section-numbering value without number-sections, got:\n{text}"
    );
}

/// `format-typst.ts`'s `wrap: none`, end to end: a long paragraph must
/// come out as a single unwrapped line, not soft-wrapped at pandoc's
/// default ~70-column width.
#[test]
fn render_document_to_file_typst_does_not_wrap_long_lines() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    let long_line = "A very long line that would definitely wrap onto multiple output \
        lines if pandocs default wrap column of seventy something characters were being \
        applied to this paragraph text right here in this test fixture document body.";
    write(&input_path, &format!("# Heading\n\n{long_line}\n"));

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output_path = project_dir.join("f.typ");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };

    render_document_to_file(
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

    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        text.contains(long_line),
        "expected the long paragraph on a single unwrapped line, got:\n{text}"
    );
}

/// pandoc-hybrid-typst Phase 1's brand bridge, end to end: an inline
/// `brand:` block in document frontmatter must reach the vendored
/// `typst-brand-yaml.lua` filter through the new `brand` filter param
/// (`TypstFilterParamsContributor`) and come out as a real
/// `#let brand-color = (...)` declaration naming the resolved color.
///
/// Revert hunk: reverting `PandocWriteStage::run`'s `typst_brand_param`
/// wiring (never calling `.with_contributor`) makes this RED — the filter
/// would see `param('brand')` as `nil` and emit `#let brand-color = (:)`,
/// which does not contain `primary`.
#[test]
fn render_document_to_file_typst_emits_brand_color_from_inline_brand_block() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\nbrand:\n  color:\n    primary: \"#1234ff\"\n---\n\n# Heading\n\nBody.\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output_path = project_dir.join("f.typ");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };

    render_document_to_file(
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

    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        text.contains("#let brand-color = ("),
        "expected a brand-color declaration, got:\n{text}"
    );
    assert!(
        text.contains("primary:"),
        "expected the primary color slot to reach the typst filter, got:\n{text}"
    );
}

/// The no-brand case must stay exactly as it was before this bridge
/// existed: no `brand` filter param at all, so the vendored filter's own
/// `brand and brand[brandMode]` guard short-circuits and emits the empty
/// `(:)` dict — not an error, not a crash.
#[test]
fn render_document_to_file_typst_without_brand_emits_empty_brand_color_dict() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(&input_path, "# Heading\n\nBody.\n");

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output_path = project_dir.join("f.typ");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };

    render_document_to_file(
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

    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        text.contains("#let brand-color = (:)"),
        "expected the empty-dict fallback with no brand configured, got:\n{text}"
    );
}

/// pandoc-hybrid-typst Phase 1's template vendoring, end to end: pandoc
/// must actually use the vendored 8-partial doctemplate (`--template`
/// pointing at the materialized `template.typ`), not its own bundled
/// default typst template. The vendored template's `typst-show.typ`
/// partial wraps the body in a real `#show: doc => article(...)` call and
/// carries the document title into `article`'s `title:` argument — the
/// bundled default template does neither (a bare, template-less-typst
/// pandoc render has no `article()` function at all).
///
/// Revert hunk: reverting the `typst_template_path`/`--template` wiring in
/// `PandocWriteStage::run` makes this RED — pandoc falls back to its own
/// default typst template, which never emits `article(`.
#[test]
fn render_document_to_file_typst_uses_vendored_template() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ntitle: Vendored Template Check\n---\n\n# Heading\n\nBody.\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output_path = project_dir.join("f.typ");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };

    render_document_to_file(
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

    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        text.contains("#show: doc => article("),
        "expected the vendored template's article() wrapper, got:\n{text}"
    );
    assert!(
        text.contains("title: [Vendored Template Check]"),
        "expected the document title threaded into article()'s title: argument, got:\n{text}"
    );
}

/// pandoc-hybrid-typst Phase 1's two deferred crossref/numbering
/// verification bullets, now reachable with the template staged: a
/// crossref'd figure and a crossref'd equation must both resolve through
/// Typst's own compiler-native numbering, not baked text.
///
/// - The `@fig-x`/`@eq-einstein` references must come out as
///   `crossref/refs.lua`'s `#ref(<label>, supplement: [...])` — the shim's
///   `route_crossref_resolved_ref` fix (earlier in this plan) — not a
///   static `Figure 1`-shaped string.
/// - The equation must come out as
///   `#math.equation(numbering: equation-numbering, ...)`, consuming the
///   `equation-numbering` symbol `numbering.typ` (now vendored) defines,
///   confirming `crossref/equations.lua`'s `isTypstOutput()` branch
///   actually engages (`route_equation`, Route N) rather than falling
///   through to a bare unlabeled `texmath` equation.
///
/// Revert hunk: reverting the template-vendoring `--template` wiring
/// makes this RED — pandoc's own bundled default typst template has no
/// `equation-numbering` symbol, so a real compile would fail outright
/// (this test doesn't compile, but the symbol's absence from `.typ`
/// source alone is diagnostic).
#[test]
fn render_document_to_file_typst_crossref_figure_and_equation_use_native_numbering() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(&project_dir.join("img.png"), "");
    write(
        &input_path,
        "---\ntitle: Crossref Check\n---\n\nSee @fig-x and @eq-einstein.\n\n![A caption.](img.png){#fig-x}\n\n$$\ne = mc^2\n$$ {#eq-einstein}\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output_path = project_dir.join("f.typ");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };

    render_document_to_file(
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

    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        text.contains("#ref(<fig-x>, supplement: [Figure])"),
        "expected a native #ref() call for the figure crossref, got:\n{text}"
    );
    assert!(
        text.contains("#ref(<eq-einstein>, supplement: [Equation])"),
        "expected a native #ref() call for the equation crossref, got:\n{text}"
    );
    assert!(
        text.contains("#math.equation(") && text.contains("numbering: equation-numbering"),
        "expected the equation to use Typst's native numbering, got:\n{text}"
    );
    assert!(
        !text.contains("Figure\u{a0}1") && !text.contains("Equation\u{a0}1"),
        "crossref citations must not bake a static number, got:\n{text}"
    );
}

/// pandoc-hybrid-typst Phase 1's "Pandoc-defaults forwarding allow-list"
/// bullet, `columns` entry (`format-typst.ts:124-129`): `format.typst.columns`
/// is typst-specific multi-column body layout. `resolve_format_config`
/// already flattens `format: typst: columns:` into a plain top-level
/// `columns` key during metadata merge — this test confirms that flattened
/// value survives all the way to `doc.ast.meta` and threads through
/// `page.typ`'s `$columns$` template variable with no additional Rust code
/// needed (unlike Q1, Q2 has no separate `format.pandoc` CLI-options dict
/// distinct from the document's own Meta block, so there is nothing to
/// "move" — the generic merge-time flattening already lands the value where
/// the template reads it).
#[test]
fn render_document_to_file_typst_forwards_columns_metadata() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ntitle: Columns Check\nformat:\n  typst:\n    columns: 2\n---\n\nBody.\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output_path = project_dir.join("f.typ");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };

    render_document_to_file(
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

    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        text.contains("columns: 2,"),
        "expected page.typ's $columns$ variable to substitute the configured value, got:\n{text}"
    );
}

/// pandoc-hybrid-typst Phase 1's "Pandoc-defaults forwarding allow-list"
/// bullet, `template` entry: a user-configured `format.typst.template`
/// (already flattened to a plain top-level `template` key by
/// `resolve_format_config`, and marked `ConfigValueKind::Path` by
/// `FORMAT_PATH_KEYS`) must replace the vendored `template.typ`, mirroring
/// Q1's `userTemplate` (`command/render/pandoc.ts:784-810`) — while the
/// vendored partials stay staged alongside it, so a custom template can
/// still reference them if it wants to.
#[test]
fn render_document_to_file_typst_user_template_overrides_vendored_template() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    let custom_template_path = project_dir.join("custom.typ");
    write(&custom_template_path, "// custom template marker\n$body$\n");
    write(
        &input_path,
        "---\ntitle: Custom Template Check\nformat:\n  typst:\n    template: custom.typ\n---\n\nBody text.\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output_path = project_dir.join("f.typ");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };

    render_document_to_file(
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

    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        text.contains("custom template marker"),
        "expected the user-supplied template to be used instead of the vendored one, got:\n{text}"
    );
    assert!(
        !text.contains("#show: doc => article("),
        "the vendored template's article() wrapper should not appear when a user template is set, got:\n{text}"
    );
}

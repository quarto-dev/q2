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

/// Margin settings must reach the vendored Typst Lua filters as
/// `QUARTO_FILTER_PARAMS`, rather than being interpreted by Pandoc as
/// unsupported CLI options. Both the layout Meta filter and the post Cite
/// filter consume these values, so assert their observable Typst output.
#[test]
fn render_document_to_file_typst_forwards_margin_locations_to_lua_filters() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ncitation-location: margin\nreference-location: margin\nbibliography: refs.bib\n---\n\n\
         A citation [@sample2020] and a footnote.^[Margin note.]\n",
    );
    write(
        &project_dir.join("refs.bib"),
        "@article{sample2020, author = {Sample, Alice}, title = {Example}, journal = {Journal}, year = {2020}}\n",
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
        text.contains("column-sidenote"),
        "reference-location: margin should emit the footnote show rule, got:\n{text}"
    );
    assert!(
        text.contains("form: \"full\""),
        "citation-location: margin should emit a full citation in the margin, got:\n{text}"
    );
}

/// `citeproc: true` must reach `quarto.doc.cite_method()` (via the new
/// `cite-method` filter param) so `quarto-post/typst.lua`'s margin-citation
/// `Cite` handler uses the pre-rendered citeproc bibliography entry in the
/// margin note instead of a bare native `#cite(<id>, form: "full")` call.
///
/// Revert hunk: reverting `PandocWriteStage::run`'s `typst_cite_method`
/// wiring (never calling `.with_contributor` with it, or dropping the
/// `cite-method` blob insertion in `TypstFilterParamsContributor`) makes
/// this RED — `quarto.doc.cite_method()` would see `nil`, and the margin
/// note would fall back to the native `#cite(...)` call this test asserts
/// is absent.
#[test]
fn render_document_to_file_typst_citeproc_true_uses_citeproc_bibliography_in_margin() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ncitation-location: margin\nbibliography: refs.bib\nciteproc: true\n---\n\n\
         A citation [@sample2020].\n",
    );
    write(
        &project_dir.join("refs.bib"),
        "@article{sample2020, author = {Sample, Alice}, title = {Example}, journal = {Journal}, year = {2020}}\n",
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
        !text.contains("#cite(<sample2020>"),
        "citeproc: true should not fall back to the native #cite(...) call, got:\n{text}"
    );
    assert!(
        text.contains("Sample, Alice. 2020."),
        "citeproc: true should emit the citeproc-rendered bibliography entry in the margin, got:\n{text}"
    );
}

/// Renders `frontmatter` + one citation to Typst source and returns it.
fn render_citation_to_typst(frontmatter: &str) -> String {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        &format!("---\n{frontmatter}bibliography: refs.bib\n---\n\nA citation [@sample2020].\n"),
    );
    write(
        &project_dir.join("refs.bib"),
        "@article{sample2020, author = {Sample, Alice}, title = {Example}, journal = {Journal}, year = {2020}}\n",
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
    std::fs::read_to_string(&output_path).unwrap()
}

/// bd-ysqekrm2 / bd-wjn7jdzw: `citeproc: true` on a plain (non-margin)
/// Typst document resolves citations through Q2's citeproc filter and
/// leaves nothing for Typst's native citation machinery: the citation is
/// rendered text, the bibliography is the `<refs>` div citeproc built, and
/// the pandoc writer (run as `typst-citations`) emits no `#bibliography()`.
///
/// Revert hunks: dropping `apply_citeproc_shorthand` from
/// `UserFiltersStage::pre` leaves the native `@sample2020`; dropping the
/// `-citations` writer suffix in `PandocWriteStage` brings back a trailing
/// `#bibliography(...)`.
#[test]
fn render_document_to_file_typst_citeproc_true_resolves_citations_without_native_bibliography() {
    let text = render_citation_to_typst("citeproc: true\n");
    assert!(
        text.contains("(Sample 2020)"),
        "citeproc: true should render the citation as text, got:\n{text}"
    );
    assert!(
        !text.contains("@sample2020"),
        "citeproc: true must not leave a native @key citation, got:\n{text}"
    );
    assert!(
        text.contains("<refs>") && text.contains("<ref-sample2020>"),
        "citeproc: true should emit citeproc's labeled bibliography div, got:\n{text}"
    );
    assert!(
        !text.contains("#bibliography("),
        "citeproc: true must not also emit a native #bibliography(), got:\n{text}"
    );
}

/// bd-ysqekrm2: without `citeproc: true` the document stays on Typst's
/// native citation path (`@key` plus a `#bibliography()` call) — the
/// default is unchanged.
#[test]
fn render_document_to_file_typst_without_citeproc_keeps_native_citations() {
    let text = render_citation_to_typst("");
    assert!(text.contains("@sample2020"), "got:\n{text}");
    assert!(text.contains("#bibliography("), "got:\n{text}");
}

/// bd-ysqekrm2: an explicit `filters: [quarto, citeproc]` puts citeproc in
/// the post group, which `PandocWriteStage`'s hybrid leg used to drop
/// without running it; it now resolves the citation like the shorthand.
#[test]
fn render_document_to_file_typst_explicit_post_citeproc_filter_resolves_citations() {
    let text = render_citation_to_typst("filters: [quarto, citeproc]\n");
    assert!(text.contains("(Sample 2020)"), "got:\n{text}");
    assert!(!text.contains("#bibliography("), "got:\n{text}");
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

/// pandoc-hybrid-typst Phase 2's `typst-available-fonts` filter param
/// (deferred by Phase 1, implemented in this session): a `font-family` CSS
/// property naming an unavailable font must be dropped from the emitted
/// `#set text(font: (...))` list, keeping only the font(s) `typst fonts`
/// actually reports — the issue-#12556 font-fallback-filtering workaround
/// vendored in `filters/modules/typst_css.lua`. "Font Awesome 6 Free" is
/// real (one of the 5 vendored packages' embedded fonts); "Some Totally
/// Fake Font" is not, so this is exactly the case `translate_font_family_list`
/// exists to handle.
#[test]
fn render_document_to_file_typst_filters_unavailable_font_family() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ntitle: Font Filter Check\n---\n\n\
         ::: {style=\"font-family: 'Font Awesome 6 Free', 'Some Totally Fake Font'\"}\n\
         Styled text.\n\
         :::\n",
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
        text.contains("#set text(font: (\"Font Awesome 6 Free\",));"),
        "expected only the available font to survive filtering, got:\n{text}"
    );
    assert!(
        !text.contains("Some Totally Fake Font"),
        "the unavailable font must not appear in the emitted font list, got:\n{text}"
    );
}

/// `mediabag-dir` end to end: a `data:image/...;base64,...` image forces
/// pandoc to resolve it into its own mediabag
/// (`render_typst_fixups`'s `Image` handler in `quarto-post/typst.lua`
/// calls `resolve_image_from_url`), and `quarto-finalize/mediabag.lua`'s
/// `Image` handler then writes it back out via
/// `modules/mediabag.lua`'s `write_mediabag_entry`, which reads
/// `param("mediabag-dir", nil)`. Before the `mediabag-dir` filter param was
/// wired (`FilterParamsBuilder::insert_mediabag_dir`), that call returned
/// `nil` and `write_mediabag_entry` crashed inside
/// `pandoc.path.join{nil, src}` with "string expected, got nil" — not
/// typst-specific (the finalize filter is unconditional for every
/// non-Office Pandoc-hybrid format), but reproduced here through typst
/// since that's the format this epic's fixtures exercise it through
/// (`crossref-grand-finale.qmd`'s remote-image fixture, out of reach here
/// without network access — a local `data:` URI triggers the identical
/// code path with no network dependency).
///
/// Revert hunk: removing `mediabag_dir`/`insert_mediabag_dir` wiring in
/// `PandocWriteStage`/`FilterParamsBuilder` makes this RED (pandoc exits
/// non-zero, `render_document_to_file` returns an error instead of `Ok`).
#[test]
fn render_document_to_file_typst_resolves_data_uri_image_via_mediabag() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    // A 1x1 transparent PNG, base64-encoded — small enough to inline, real
    // enough that `should_mediabag`'s `data:image/.+;base64,(.+)` pattern
    // matches and pandoc actually decodes it into the mediabag.
    write(
        &input_path,
        "---\ntitle: Mediabag Check\n---\n\n\
         ![alt text](data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=)\n",
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
    .expect("typst render should succeed instead of crashing in modules/mediabag.lua");

    let mediabag_dir = project_dir.join("f_files").join("mediabag");
    assert!(
        mediabag_dir.is_dir(),
        "expected mediabag-dir to be created at {}",
        mediabag_dir.display()
    );
    let entries: Vec<_> = std::fs::read_dir(&mediabag_dir)
        .expect("mediabag dir should be readable")
        .filter_map(|e| e.ok())
        .collect();
    assert!(
        !entries.is_empty(),
        "expected at least one file written into {}",
        mediabag_dir.display()
    );

    let text = std::fs::read_to_string(&output_path).unwrap();
    assert!(
        text.contains("f_files/mediabag") || text.contains("f_files\\mediabag"),
        "expected the typst output to reference the written mediabag file, got:\n{text}"
    );
}

/// A raw-HTML table holding a data-URI `<img>` over 2000 characters must
/// not leak the placeholder `juice()` swaps in for it (normalize/
/// astpipeline.lua) into the Typst source.
///
/// `juice.ts` does not ship with q2, so the juice step always falls back;
/// that fallback used to return the placeholder-substituted HTML, leaving a
/// bare UUID as the `image()` path ("file not found" from Typst). Short
/// data-URIs are not substituted, so one of each size in the same table
/// covers both paths.
///
/// Revert hunk: returning `htmltext` (not `restore_data_uris(htmltext)`)
/// from the `not ok` branch of `juice()` makes this RED.
#[test]
fn render_document_to_file_typst_raw_html_table_long_data_uri_images_reach_mediabag() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    let short_png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";
    // Over the 2000-char threshold; decodes to bytes distinct from `short_png`.
    let long_png = format!("iVBORw0KGgo{}", "A".repeat(2400));
    write(
        &input_path,
        &format!(
            "---\ntitle: Juice Check\n---\n\n```{{=html}}\n<table><tr><td>\n\
             <img src=\"data:image/png;base64,{short_png}\">\n</td><td>\n\
             <img src=\"data:image/png;base64,{long_png}\">\n</td></tr></table>\n```\n"
        ),
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
        !text.contains("273dae7e-3633-4385-9b0c-203d2d7a2d37"),
        "juice placeholder leaked into typst output:\n{text}"
    );
    let image_calls = text.matches("image(\"").count();
    assert_eq!(image_calls, 2, "expected two image() calls, got:\n{text}");
    assert_eq!(
        text.matches("f_files/mediabag/").count(),
        2,
        "both images should point into the mediabag, got:\n{text}"
    );
    let n = std::fs::read_dir(project_dir.join("f_files").join("mediabag"))
        .unwrap()
        .count();
    assert_eq!(n, 2, "expected two mediabag files");
}

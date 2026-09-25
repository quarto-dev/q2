/*
 * tests/integration/book_preview.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P8: book-aware `q2 preview` — native E2E coverage of
 * `RenderToPreviewAstRenderer` against real multi-chapter book fixtures.
 *
 * Companion to `book_multifile_html.rs` (the real, non-preview book
 * render, whose fixtures this file's `write_minibook` mirrors) and
 * `render_page_in_project.rs` (the preview-orchestrator harness this
 * file's `render_active_page_preview`/`ast_json` helpers mirror).
 *
 * A previewed chapter never runs `crossref-render` — it is on
 * `Q2_PREVIEW_TRANSFORM_EXCLUDED` (pipeline.rs), since preview needs the
 * still-structured `CrossrefResolvedRef` node, not flattened HTML. So
 * unlike `book_multifile_html.rs`'s HTML-string assertions
 * (`"Figure\u{a0}2.1"`, `"header-section-number"`), these tests inspect
 * the AST JSON's structured data directly: the `CrossrefResolvedRef`
 * custom node's `plain_data` (`resolved`, `resolved_number`,
 * `owning_chapter_path`) and the `Header` block's own `number` kv
 * (stashed by `CrossrefIndexTransform`, which does run in preview).
 *
 * `plain_data` is carried as a JSON-encoded string inside the
 * `data-custom-data` attribute (see `pampa/src/writers/json.rs`), so
 * when the whole AST is serialized the embedded quotes come out
 * backslash-escaped. Assertions on `plain_data` keys/values therefore
 * use bare-word `contains` (no manual quoting) — the same convention
 * `render_page_in_project.rs`'s preview tests already use for
 * `__quarto_custom_node`/`data-custom-type`/`Callout`. Assertions on a
 * native `Header`'s own `attr` kv (not double-encoded) use the exact
 * Pandoc-JSON pair shape `["number","2"]`.
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};
use quarto_core::project::pass2_renderer::{RenderToPreviewAstRenderer, WasmPassTwoOutput};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// A chapter file with an H1, a captioned figure (code-block shorthand —
/// matches `book_multifile_html.rs`'s `chapter_source`: the div +
/// `#| fig-cap` shape does not attach captions in q2's render path
/// today), and prose around it.
fn chapter_source(title: &str, fig_id: &str, fig_cap: &str, refs: &str) -> String {
    format!(
        "# {title}\n\n{refs}\n\n```{{.python}}\n#| label: {fig_id}\n#| fig-cap: {fig_cap}\nx = 1\n```\n"
    )
}

/// The same 3-chapter + 1-appendix book shape `book_multifile_html.rs`
/// uses for its P4/P5 real-render fixtures: `index.qmd` (unnumbered
/// welcome page), `ch1.qmd` (fig-one, chapter 1), `ch2.qmd` (fig-two,
/// own figure; a cross-chapter `@fig-one` reference back into ch1),
/// `app-a.qmd` (fig-app, letter-scoped appendix).
fn write_minibook(project_dir: &Path) {
    write(
        &project_dir.join("_quarto.yml"),
        r#"project:
  type: book

book:
  title: "Minibook"
  author: "Test Author"
  chapters:
    - index.qmd
    - ch1.qmd
    - ch2.qmd
  appendices:
    - app-a.qmd
"#,
    );
    write(
        &project_dir.join("index.qmd"),
        "# Welcome {.unnumbered}\n\nWelcome to the book.\n",
    );
    write(
        &project_dir.join("ch1.qmd"),
        &chapter_source(
            "Chapter One",
            "fig-one",
            "A figure in chapter one",
            "Chapter one's own figure is @fig-one.",
        ),
    );
    write(
        &project_dir.join("ch2.qmd"),
        &chapter_source(
            "Chapter Two",
            "fig-two",
            "A figure in chapter two",
            "Chapter two references @fig-two only; @fig-one is cross-chapter (P5).",
        ),
    );
    write(
        &project_dir.join("app-a.qmd"),
        &chapter_source(
            "Appendix Alpha",
            "fig-app",
            "A figure in the appendix",
            "The appendix references @fig-app.",
        ),
    );
}

/// Drive `ProjectPipeline<RenderToPreviewAstRenderer>` with
/// `RenderMode::ActivePage(active)` — the same harness
/// `render_page_in_project.rs`'s `render_active_page_preview` uses.
/// Duplicated locally (rather than shared via `pub` + cross-module
/// `use`) to keep this file's book-preview-specific fixtures and
/// harness self-contained, matching `book_multifile_html.rs`'s own
/// preference for local helpers over cross-file reuse.
fn render_active_page_preview(active: &Path) -> WasmPassTwoOutput {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(active, runtime.as_ref()).unwrap();
    if !project.is_single_file {
        project = ProjectContext::discover(&project.dir, runtime.as_ref()).unwrap();
    }

    let project_type = project_type_for(&project);
    let vfs_root = project.dir.join(".quarto/project-artifacts");
    let renderer = RenderToPreviewAstRenderer::new(&vfs_root);

    let format =
        Format::from_format_string("q2-preview").expect("q2-preview is a recognized pseudo-format");

    let mut pipeline = ProjectPipeline::with_renderer(
        &mut project,
        project_type,
        format,
        "q2-preview",
        runtime.clone(),
        renderer,
    )
    .with_mode(RenderMode::ActivePage(active.to_path_buf()));

    let summary = pollster::block_on(pipeline.run()).expect("q2-preview pipeline run");
    assert!(
        summary.pass1_failures.is_empty(),
        "unexpected pass-1 failures: {:?}",
        summary.pass1_failures,
    );
    assert!(
        summary.pass2_failures.is_empty(),
        "unexpected pass-2 failures: {:?}",
        summary.pass2_failures,
    );
    assert_eq!(
        summary.outputs.len(),
        1,
        "ActivePage mode should produce exactly one output"
    );
    summary.outputs.into_iter().next().unwrap()
}

fn ast_json(output: &WasmPassTwoOutput) -> &str {
    output
        .payload
        .as_ast_json()
        .expect("q2-preview renderer must produce Pass2Payload::AstJson")
}

fn snippet(s: impl AsRef<str>) -> String {
    let s = s.as_ref();
    if s.len() <= 400 {
        s.to_string()
    } else {
        format!("{}…", &s[..400])
    }
}

/// Tests-first checklist: previewing a chapter in a non-book project is
/// byte-for-byte unaffected — no chapter seed, no `resolved_number`, no
/// `owning_chapter_path`. `cross_chapter_crossref_registry` staying
/// `None` is exercised through the preview path here (P5's own no-op
/// guard for non-book renders is unit-tested elsewhere).
#[test]
fn non_book_preview_is_unaffected_by_book_preview_wiring() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: default\n",
    );
    write(
        &project_dir.join("doc.qmd"),
        &chapter_source(
            "Standalone Doc",
            "fig-solo",
            "A solo figure",
            "See @fig-solo.",
        ),
    );

    let active = canonical(&project_dir.join("doc.qmd"));
    let output = render_active_page_preview(&active);
    let json = ast_json(&output);
    let snip = || snippet(json);

    assert!(
        json.contains("__quarto_custom_node") && json.contains("fig-solo"),
        "expected a resolved CrossrefResolvedRef for fig-solo; got:\n{}",
        snip()
    );
    assert!(
        !json.contains("resolved_number"),
        "non-book preview must not compose a chapter-scoped resolved_number; got:\n{}",
        snip()
    );
    assert!(
        !json.contains("owning_chapter_path"),
        "non-book preview must never carry owning_chapter_path; got:\n{}",
        snip()
    );
    assert!(
        !json.contains("[\"number\","),
        "non-book preview's H1 must carry no book chapter-number kv; got:\n{}",
        snip()
    );
}

/// Tests-first checklist: previewing chapter 2 of a real multi-chapter
/// book fixture shows chapter-scoped numbering ("2.1", not "1") for its
/// own local figure ref, and the chapter-number data ("2") a real
/// `BookProjectType` render's title decoration is built from — both via
/// P0/P4's existing machinery, reused verbatim (see this phase's
/// Decisions). The rendered "Figure\u{a0}2.1" *text*/`header-section-
/// number` *class* themselves are `crossref-render`'s job
/// (`Q2_PREVIEW_TRANSFORM_EXCLUDED`), so this test inspects the
/// structured data those would be built from instead — the same
/// contract, one layer earlier, matching what React's
/// `CrossrefResolvedRef.tsx` actually reads.
#[test]
fn book_chapter_preview_gets_chapter_scoped_number_and_header_number_kv() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);

    let active = canonical(&project_dir.join("ch2.qmd"));
    let output = render_active_page_preview(&active);
    let json = ast_json(&output);
    let snip = || snippet(json);

    assert!(
        json.contains("resolved_number") && json.contains("2.1"),
        "ch2's own fig-two ref must compose the chapter-scoped resolved_number 2.1; got:\n{}",
        snip()
    );
    assert!(
        json.contains("[\"number\",\"2\"]"),
        "ch2's H1 must carry the book chapter-number kv (\"2\"), the same fact a real \
         render's title decoration is built from; got:\n{}",
        snip()
    );
}

/// Tests-first checklist: an `.unnumbered` chapter previews with no
/// chapter-number seed applied — matches P0/P4's existing unnumbered-
/// chapter behavior (no seed → no `resolved_number`, no `number` kv),
/// exercised here through the preview path specifically.
#[test]
fn unnumbered_chapter_preview_gets_no_seed_or_number_kv() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);
    write(
        &project_dir.join("_quarto.yml"),
        r#"project:
  type: book

book:
  title: "Minibook"
  author: "Test Author"
  chapters:
    - index.qmd
    - ch1.qmd
    - ch1b.qmd
    - ch2.qmd
  appendices:
    - app-a.qmd
"#,
    );
    write(
        &project_dir.join("ch1b.qmd"),
        &chapter_source(
            "Interlude {.unnumbered}",
            "fig-interlude",
            "A figure in the interlude",
            "The interlude references @fig-interlude.",
        ),
    );

    let active = canonical(&project_dir.join("ch1b.qmd"));
    let output = render_active_page_preview(&active);
    let json = ast_json(&output);
    let snip = || snippet(json);

    assert!(
        json.contains("fig-interlude") && json.contains("__quarto_custom_node"),
        "expected a resolved CrossrefResolvedRef for the interlude's own figure; got:\n{}",
        snip()
    );
    assert!(
        !json.contains("resolved_number"),
        "an unnumbered chapter previews with no seed, so no resolved_number is composed; got:\n{}",
        snip()
    );
    assert!(
        !json.contains("[\"number\","),
        "an unnumbered chapter's H1 must carry no chapter-number kv; got:\n{}",
        snip()
    );
}

/// Tests-first checklist (baseline): a cross-chapter `@ref` to an id no
/// chapter defines degrades to the existing unresolved-placeholder
/// contract (`resolved: false`, no crash) — pins today's floor so the
/// StaticProjectAnalyzer improvement is measurable against it.
#[test]
fn preview_unknown_cross_chapter_ref_stays_unresolved() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);
    write(
        &project_dir.join("ch2.qmd"),
        &chapter_source(
            "Chapter Two",
            "fig-two",
            "A figure in chapter two",
            "Chapter two references its own @fig-two and an unknown @fig-does-not-exist.",
        ),
    );

    let active = canonical(&project_dir.join("ch2.qmd"));
    let output = render_active_page_preview(&active);
    let json = ast_json(&output);
    let snip = || snippet(json);

    assert!(
        json.contains("fig-does-not-exist"),
        "expected the unresolvable identifier to still appear in the AST; got:\n{}",
        snip()
    );
    assert!(
        json.contains(r#"resolved\":false"#),
        "an id no chapter defines must stay unresolved; got:\n{}",
        snip()
    );
}

/// Tests-first checklist: `StaticProjectAnalyzer` resolves a
/// cross-chapter `@ref` to the target chapter's approximate kind/number,
/// with that chapter's own `ChapterSeed` correctly applied — and the new
/// `owning_chapter_path` field is present (with the correct source
/// path) for that cross-chapter-resolved ref.
#[test]
fn static_analyzer_resolves_cross_chapter_ref_with_owning_chapter_path() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);

    // ch2's fixture already carries a cross-chapter @fig-one (defined in
    // ch1) — see write_minibook's chapter_source `refs` argument.
    let active = canonical(&project_dir.join("ch2.qmd"));
    let output = render_active_page_preview(&active);
    let json = ast_json(&output);
    let snip = || snippet(json);

    assert!(
        json.contains("fig-one") && json.contains("resolved_number") && json.contains("1.1"),
        "the cross-chapter ref must resolve to ch1's scoped number 1.1; got:\n{}",
        snip()
    );
    assert!(
        json.contains("owning_chapter_path") && json.contains("ch1.qmd"),
        "the cross-chapter ref must carry owning_chapter_path pointing at ch1.qmd; got:\n{}",
        snip()
    );
}

/// Tests-first checklist: an `@ref` to an engine-generated (`output:
/// asis`, un-promised — not declared via `crossref.ids`) id in another
/// chapter still degrades to the unresolved-placeholder rendering, not a
/// crash or an incorrect resolution. `StaticProjectAnalyzer` never
/// executes engines, so an id only an engine would produce is simply
/// absent from the sweep's index — identical, in outcome, to any other
/// unresolvable id.
#[test]
fn engine_generated_id_in_sibling_degrades_gracefully() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\n\
         Chapter one's own figure is @fig-one.\n\n\
         ```{.python}\n#| label: fig-one\n#| fig-cap: A figure in chapter one\nx = 1\n```\n\n\
         ```{.python}\n#| output: asis\n\
         # An engine (executed only at real-render time) would emit a\n\
         # figure with id fig-dynamic here; the static sweep never runs\n\
         # this cell, so fig-dynamic never enters the crossref index.\n\
         print('#| label: fig-dynamic')\n```\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        &chapter_source(
            "Chapter Two",
            "fig-two",
            "A figure in chapter two",
            "Chapter two references its own @fig-two and ch1's engine-generated @fig-dynamic.",
        ),
    );

    let active = canonical(&project_dir.join("ch2.qmd"));
    let output = render_active_page_preview(&active);
    let json = ast_json(&output);
    let snip = || snippet(json);

    assert!(
        json.contains("fig-dynamic"),
        "expected fig-dynamic to still appear (unresolved) in the AST; got:\n{}",
        snip()
    );
    // Loose check: fig-dynamic's own plain_data has no resolved_number
    // composed for it. fig-two (this chapter's own local ref) legitimately
    // does, so this only pins that the sweep didn't fabricate a bogus
    // resolution for the un-swept id — a full per-node parse isn't needed
    // to catch a crash-or-wrong-resolution regression here.
    assert!(
        json.contains(r#"resolved\":false"#),
        "an id only an engine could produce must stay unresolved, not crash \
         or resolve incorrectly; got:\n{}",
        snip()
    );
}

/// Tests-first checklist: a non-chapter `.qmd` file present in the
/// project directory but absent from `BookRenderItem`'s list (never
/// listed in `book.chapters`/`book.appendices`) is not swept by
/// `StaticProjectAnalyzer` and cannot be resolved as a cross-chapter
/// target — matches the design's D2/D6-precedented scoping rule (e.g.
/// an include-shortcode partial must not be treated as a chapter).
#[test]
fn non_chapter_qmd_file_is_not_swept_by_static_analyzer() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);
    // Present on disk, but never listed in `_quarto.yml`'s book config —
    // BookRenderItem's list (and therefore the sweep) must skip it.
    write(
        &project_dir.join("partial.qmd"),
        &chapter_source(
            "Not A Chapter",
            "fig-partial",
            "A figure in an unlisted partial",
            "This file is never listed as a book chapter.",
        ),
    );
    write(
        &project_dir.join("ch2.qmd"),
        &chapter_source(
            "Chapter Two",
            "fig-two",
            "A figure in chapter two",
            "Chapter two references its own @fig-two and the unlisted @fig-partial.",
        ),
    );

    let active = canonical(&project_dir.join("ch2.qmd"));
    let output = render_active_page_preview(&active);
    let json = ast_json(&output);
    let snip = || snippet(json);

    assert!(
        json.contains("fig-partial"),
        "expected the identifier to still appear (unresolved) in the AST; got:\n{}",
        snip()
    );
    assert!(
        json.contains(r#"resolved\":false"#),
        "an id defined only in an unlisted, non-chapter .qmd file must stay unresolved; got:\n{}",
        snip()
    );
}

/// Tests-first checklist: two chapters independently defining the same
/// crossref id produce a non-fatal diagnostic and a deterministic
/// (first-in-book-order) resolution in the static sweep, not a silent
/// last-wins overwrite and not a preview crash. A third chapter's
/// cross-chapter ref to the duplicated id must resolve against the
/// *first* chapter (ch1), never ch2 — pinning
/// `aggregate_chapter_inventories`'s shared merge rule through the
/// preview-specific caller.
#[test]
fn duplicate_id_across_chapters_is_non_fatal_and_first_chapter_wins() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);
    write(
        &project_dir.join("_quarto.yml"),
        r#"project:
  type: book

book:
  title: "Minibook"
  author: "Test Author"
  chapters:
    - index.qmd
    - ch1.qmd
    - ch2.qmd
    - ch3.qmd
  appendices:
    - app-a.qmd
"#,
    );
    // ch1's first figure is fig-one (order 1 -> "1.1"); fig-dup is its
    // second (order 2 -> "1.2").
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\n\
         Chapter one's own figure is @fig-one.\n\n\
         ```{.python}\n#| label: fig-one\n#| fig-cap: A figure in chapter one\nx = 1\n```\n\n\
         Chapter one also defines a duplicate-prone figure @fig-dup.\n\n\
         ```{.python}\n#| label: fig-dup\n#| fig-cap: Chapter one's shared figure\nx = 1\n```\n",
    );
    // ch2's first figure is fig-two (order 1 -> "2.1"); its own fig-dup is
    // its second (order 2 -> "2.2") — referenced locally, so this pins
    // "local always wins" too: ch2's own @fig-dup must show 2.2, never
    // ch1's 1.2, and must carry no owning_chapter_path.
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\n\
         Chapter two's own figure is @fig-two, and its own local shared-id figure is @fig-dup.\n\n\
         ```{.python}\n#| label: fig-two\n#| fig-cap: A figure in chapter two\nx = 1\n```\n\n\
         ```{.python}\n#| label: fig-dup\n#| fig-cap: Chapter two's shared figure\nx = 1\n```\n",
    );
    write(
        &project_dir.join("ch3.qmd"),
        "# Chapter Three\n\n\
         This chapter references the shared figure @fig-dup, defined in both chapter one \
         and chapter two; by the first-in-book-order rule it must resolve against chapter one.\n",
    );

    // ch2: local resolution must win over the (would-be) sibling entry.
    let ch2_active = canonical(&project_dir.join("ch2.qmd"));
    let ch2_output = render_active_page_preview(&ch2_active);
    let ch2_json = ast_json(&ch2_output);
    assert!(
        ch2_json.contains("resolved_number") && ch2_json.contains("2.2"),
        "ch2's own local @fig-dup must resolve to its own chapter-scoped number 2.2; got:\n{}",
        snippet(ch2_json)
    );
    assert!(
        !ch2_json.contains("owning_chapter_path"),
        "ch2's own local resolution must never carry owning_chapter_path; got:\n{}",
        snippet(ch2_json)
    );

    // ch3: cross-chapter ref must resolve against ch1 (first-in-book-
    // order), not ch2, and the sweep's merge diagnostic must surface.
    let ch3_active = canonical(&project_dir.join("ch3.qmd"));
    let ch3_output = render_active_page_preview(&ch3_active);
    let ch3_json = ast_json(&ch3_output);
    assert!(
        ch3_json.contains("resolved_number") && ch3_json.contains("1.2"),
        "ch3's cross-chapter @fig-dup must resolve to ch1's number 1.2 (first wins); got:\n{}",
        snippet(ch3_json)
    );
    assert!(
        ch3_json.contains("owning_chapter_path") && ch3_json.contains("ch1.qmd"),
        "ch3's cross-chapter @fig-dup must point at ch1.qmd, not ch2.qmd; got:\n{}",
        snippet(ch3_json)
    );
    assert!(
        ch3_output
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-15-2")),
        "the duplicate-id-across-chapters diagnostic (Q-15-2) must surface even though \
         preview still renders successfully (non-fatal); got: {:?}",
        ch3_output.diagnostics
    );
}

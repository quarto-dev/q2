/*
 * tests/integration/book_multifile_html.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P4: multi-file HTML books. Each chapter renders
 * independently (ordinary Pass 2) with one injected fact — its book
 * chapter number / appendix flag — so display numbers are chapter-local
 * ("Figure 2.1", "Figure A.1") while the per-file counter stays flat.
 *
 * The numbering contract is the one observed against real Q1 renders
 * (plan appendix: `scratchpad/minibook` + the unnumbered-interlude
 * experiment): chapter 1 → 1.1, chapter 2 → 2.1, appendix → A.1; an
 * unnumbered chapter consumes no slot and numbers its own figures flat.
 * Cross-chapter `@ref`s deliberately ship as the visible unresolved
 * placeholder (P5 replaces it with the project-wide registry).
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
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

/// Count rendered .html outputs under the book's output directory.
fn count_html_outputs(project_dir: &Path) -> usize {
    let mut n = 0;
    let out = project_dir.join("_book");
    let mut stack = vec![out];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "html") {
                n += 1;
            }
        }
    }
    n
}

/// A chapter file with an H1, a captioned figure (code-block shorthand —
/// the div + `#| fig-cap` shape does not attach captions in q2's render
/// path today, verified against a standalone non-book render), and prose
/// around it.
fn chapter_source(title: &str, fig_id: &str, fig_cap: &str, refs: &str) -> String {
    format!(
        "# {title}\n\n{refs}\n\n```{{.python}}\n#| label: {fig_id}\n#| fig-cap: {fig_cap}\nx = 1\n```\n"
    )
}

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
    // Q1's book template shape: the home page is an unnumbered H1 —
    // `chapter_is_numbered` treats a string front-matter title as
    // numbered, which would consume chapter slot 1 and push every
    // chapter's number up (Q1-faithful, but not the experiment's book).
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

/// Render `project_dir` as a multi-file HTML book and return the rendered
/// HTML text of one output file, found by its stem (e.g. `"ch2"`).
fn render_and_read(project_dir: &Path, stem: &str) -> String {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(project_dir, runtime.as_ref()).unwrap();
    let project_type = project_type_for(&project);
    let format = Format::from_format_string("html").unwrap();
    let options = RenderToFileOptions::default();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        "html",
        &options,
        runtime,
    );
    let summary = pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("run_with_book_support failed: {e}"));
    assert!(
        !summary.outputs.is_empty(),
        "expected at least one output, got {:?}",
        summary.outputs
    );
    let output_path = summary
        .outputs
        .iter()
        .map(|o| &o.output_path)
        .find(|p| {
            p.file_stem().is_some_and(|s| s == stem) && p.extension().is_some_and(|e| e == "html")
        })
        .unwrap_or_else(|| panic!("no html output with stem {stem} in {:?}", summary.outputs));
    std::fs::read_to_string(output_path).unwrap()
}

/// P4 checklist: a 3-chapter + 1-appendix book renders N separate files
/// with per-chapter-local numbers, matching the `scratchpad/minibook`
/// experiment's observed Q1 output exactly: chapter 1 → 1.1, chapter 2 →
/// 2.1, appendix → A.1 (a separate letter sequence, not "4").
#[test]
fn three_chapter_book_renders_separate_files_with_chapter_local_numbers() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);

    let ch1 = render_and_read(&project_dir, "ch1");
    let ch2 = render_and_read(&project_dir, "ch2");
    let app_a = render_and_read(&project_dir, "app-a");

    // One output per chapter file (the multi-file-ness itself).
    assert_eq!(count_html_outputs(&project_dir), 4, "index + 3 chapters");

    // Chapter 1: H1 numbers "1"; its own figure and in-text ref are 1.1.
    assert!(
        ch1.contains("header-section-number\">1<"),
        "ch1 H1 must number 1: {}",
        ch1
    );
    assert!(
        ch1.contains("Figure\u{a0}1.1: A figure in chapter one"),
        "ch1 caption must be chapter-scoped 1.1: {}",
        ch1
    );
    assert!(
        ch1.contains(">Figure\u{a0}1.1</a>"),
        "ch1's same-chapter ref must show 1.1: {}",
        ch1
    );

    // Chapter 2: H1 numbers "2" — this is the assertion the flat counter
    // fails (today it renders "1"); its figure is 2.1, not 1.1 or 2.2.
    assert!(
        ch2.contains("header-section-number\">2<"),
        "ch2 H1 must number 2 (seeded chapter number): {}",
        ch2
    );
    assert!(
        ch2.contains("Figure\u{a0}2.1: A figure in chapter two"),
        "ch2 caption must be chapter-scoped 2.1: {}",
        ch2
    );
    assert!(
        !ch2.contains("Figure\u{a0}1"),
        "ch2 must not show chapter 1's numbering: {}",
        ch2
    );

    // Appendix: "Appendix A —" H1 shape; the figure letters A.1 —
    // a separate sequence from the numeric chapters, not "3.1"/"4.1".
    assert!(
        app_a.contains("header-section-number\">A<"),
        "appendix H1 must number as letter A: {}",
        app_a
    );
    assert!(
        app_a.contains("Appendix <span class=\"header-section-number\">A</span> —"),
        "appendix H1 must carry the 'Appendix A —' prefix shape: {}",
        app_a
    );
    assert!(
        app_a.contains("Figure\u{a0}A.1: A figure in the appendix"),
        "appendix caption must be letter-scoped A.1: {}",
        app_a
    );
}

/// P4 checklist: an unnumbered chapter between two numbered ones (a)
/// consumes no chapter-number slot — chapter 2's numbers stay 2/2.1, not
/// 3/3.1 — and (b) numbers its own figures flat, plain `Figure 1`
/// (observed against the real unnumbered-interlude experiment; not
/// `Figure 0.1`, not chapter-scoped in any form).
#[test]
fn unnumbered_chapter_consumes_no_slot_and_numbers_flat() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);
    // Insert the unnumbered interlude between ch1 and ch2.
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

    let ch1b = render_and_read(&project_dir, "ch1b");
    let ch2 = render_and_read(&project_dir, "ch2");

    // (a) The unnumbered chapter is invisible to the chapter sequence:
    // chapter 2 still numbers 2 / 2.1.
    assert!(
        ch2.contains("header-section-number\">2<"),
        "ch2 H1 must still number 2 — the unnumbered interlude consumed no slot: {}",
        ch2
    );
    assert!(
        ch2.contains("Figure\u{a0}2.1: A figure in chapter two"),
        "ch2 caption must stay 2.1, not 3.1: {}",
        ch2
    );

    // (b) The interlude's own figure is flat "Figure 1" — no seed, no
    // chapter scoping of any kind.
    assert!(
        !ch1b.contains("header-section-number"),
        "the unnumbered chapter's H1 must carry no number: {}",
        ch1b
    );
    assert!(
        ch1b.contains("Figure\u{a0}1: A figure in the interlude"),
        "the interlude's own figure must number flat 1: {}",
        ch1b
    );
}

/// P4 checklist: a same-chapter `@ref` resolves to a working link with
/// the correct (chapter-scoped) number.
#[test]
fn same_chapter_ref_resolves_to_working_link() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);
    let ch2 = render_and_read(&project_dir, "ch2");

    // The link to the same chapter's figure carries the scoped number
    // and a working in-page anchor target. (The minibook's ch2 also
    // holds a deliberate cross-chapter ref — `?fig-one?` — whose
    // unresolved marker is pinned by the next test, so the "not
    // unresolved" check here is scoped to the same-chapter link.)
    assert!(
        ch2.contains("href=\"#fig-two\""),
        "the same-chapter ref must link to the figure anchor: {}",
        ch2
    );
    assert!(
        ch2.contains(">Figure\u{a0}2.1</a>"),
        "the same-chapter ref must show the scoped number: {}",
        ch2
    );
    assert!(
        ch2.contains("<a href=\"#fig-two\" class=\"quarto-xref\">"),
        "the same-chapter link must not carry the unresolved-ref class: {}",
        ch2
    );
}

/// P4 checklist: sidebar navigation generated from P1's config
/// translation renders correctly for a real multi-chapter book — the
/// sidebar itself, the book title, one decorated link per chapter
/// (Q1's `numberChapterHtmlNav` spans, labels sourced from each page's
/// front-matter title), the active state on the current page, and the
/// Appendices collapsible section. Navbar: books are sidebar-style in
/// Q1 too — nothing to assert. Download-tools: P1's `download_tools`
/// entries are inert config until a sidebar-tools renderer exists
/// (bd-fod3), so they have no HTML surface to assert here.
#[test]
fn sidebar_renders_decorated_chapter_links_for_multichapter_book() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);
    // Front-matter titles give the sidebar labels to decorate (without
    // one, an entry stays a bare-href link — the documented fallback the
    // other tests render).
    write(
        &project_dir.join("ch1.qmd"),
        &format!(
            "---\ntitle: \"Chapter One\"\n---\n\n{}",
            chapter_source(
                "Chapter One",
                "fig-one",
                "A figure in chapter one",
                "Chapter one's own figure is @fig-one.",
            )
        ),
    );
    write(
        &project_dir.join("ch2.qmd"),
        &format!(
            "---\ntitle: \"Chapter Two\"\n---\n\n{}",
            chapter_source(
                "Chapter Two",
                "fig-two",
                "A figure in chapter two",
                "Chapter two references @fig-two only; @fig-one is cross-chapter (P5).",
            )
        ),
    );
    write(
        &project_dir.join("app-a.qmd"),
        &format!(
            "---\ntitle: \"Appendix Alpha\"\n---\n\n{}",
            chapter_source(
                "Appendix Alpha",
                "fig-app",
                "A figure in the appendix",
                "The appendix references @fig-app.",
            )
        ),
    );

    let ch1 = render_and_read(&project_dir, "ch1");

    // The sidebar exists and carries the book title.
    assert!(
        ch1.contains("id=\"quarto-sidebar\""),
        "the sidebar must render: {}",
        ch1
    );
    assert!(
        ch1.contains("sidebar-title"),
        "the sidebar title block must render: {}",
        ch1
    );
    assert!(
        ch1.contains(">Minibook</a>"),
        "the sidebar must show the book title: {}",
        ch1
    );

    // One link per chapter, the current page's marked active.
    assert!(
        ch1.contains("href=\"ch1.html\" class=\"sidebar-item-text sidebar-link active\""),
        "the current chapter's sidebar link must be active: {}",
        ch1
    );
    assert!(
        ch1.contains("href=\"ch2.html\" class=\"sidebar-item-text sidebar-link\""),
        "the other chapter's sidebar link must render non-active: {}",
        ch1
    );

    // Decorated labels: Q1's numberChapterHtmlNav spans, chapter 1
    // numeric and the appendix letter-scoped inside its section.
    assert!(
        ch1.contains("<span class=\"chapter-number\">1</span>\u{a0} <span class=\"chapter-title\">Chapter One</span>"),
        "chapter 1's sidebar label must be decorated '1 Chapter One': {}",
        ch1
    );
    assert!(
        ch1.contains("<span class=\"chapter-number\">A</span>\u{a0} <span class=\"chapter-title\">Appendix Alpha</span>"),
        "the appendix's sidebar label must be letter-decorated: {}",
        ch1
    );
    assert!(
        ch1.contains("href=\"app-a.html\"") && ch1.contains("Appendices"),
        "the appendix must appear under an Appendices section: {}",
        ch1
    );
}

/// P4 checklist: Q1's `book.scss` (shipped by `bookScssBundle()`) appears
/// in the rendered page's compiled CSS — the single rule that colors the
/// decorated sidebar chapter numbers, `.sidebar-item .chapter-number {
/// color: $body-color; }`, with `$body-color` resolved to a concrete
/// value by the Bootstrap compile.
#[test]
fn book_scss_rules_appear_in_compiled_theme_css() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);
    let ch1 = render_and_read(&project_dir, "ch1");

    // The page links the fingerprinted theme CSS...
    assert!(
        ch1.contains("quarto-theme-"),
        "ch1 must link its compiled theme CSS: {}",
        ch1
    );

    // ...and that compiled file — a book project publishes the theme as
    // `quarto/quarto-theme-<fp>.css` under the output dir's `site_libs` —
    // carries the book rule with `$body-color` resolved to a concrete
    // color declaration.
    let quarto_lib = project_dir.join("_book").join("site_libs").join("quarto");
    let mut found = false;
    let mut snippet = String::new();
    for entry in std::fs::read_dir(&quarto_lib).unwrap().flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "css")
            && let Ok(css) = std::fs::read_to_string(&p)
            && let Some(idx) = css.find(".sidebar-item .chapter-number")
        {
            found = true;
            let window_end = (idx + 200).min(css.len());
            snippet = css[idx..window_end].to_string();
        }
    }
    assert!(
        found,
        "the compiled theme CSS must carry the book.scss rule; looked in {}",
        quarto_lib.display()
    );
    assert!(
        snippet.contains("color:"),
        "the book rule must compile with $body-color resolved to a color declaration: {snippet}"
    );
}

/// A real (1×1 transparent) PNG, so every consumer of the file — the
/// resource copier, anything that sniffs bytes — sees a valid image.
const COVER_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// P4 checklist: the book's cover image renders on the index page, inside
/// the first `<section>` right after its header — Q1's net shape after its
/// DOM postprocessor moves the prepended `<p>` into place — but achieved
/// purely by AST insertion ahead of `SectionizeTransform`, with no
/// DOM-level fixup. The file itself must be copied into the output tree
/// (via the ordinary resource-collector, since the Image is now in the
/// AST), so the `src` resolves without any special-casing.
#[test]
fn cover_image_renders_inside_preface_first_section() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);
    std::fs::write(project_dir.join("cover.png"), COVER_PNG).unwrap();
    // Q1's primary source: `book.cover-image` in `_quarto.yml`, with an
    // accessible-name override in `book.cover-image-alt`.
    write(
        &project_dir.join("_quarto.yml"),
        r#"project:
  type: book

book:
  title: "Minibook"
  author: "Test Author"
  cover-image: cover.png
  cover-image-alt: "Book cover"
  chapters:
    - index.qmd
    - ch1.qmd
    - ch2.qmd
  appendices:
    - app-a.qmd
"#,
    );

    let index = render_and_read(&project_dir, "index");

    // The image is present, carrying Q1's classes, the config path as
    // src, and the book title as the img title.
    let img_pos = index
        .find("quarto-cover-image")
        .unwrap_or_else(|| panic!("the cover image must render on the index page: {index}"));
    let tag_start = index[..img_pos]
        .rfind("<img")
        .unwrap_or_else(|| panic!("quarto-cover-image must sit inside an <img> tag: {index}"));
    let tag_end = tag_start
        + index[tag_start..]
            .find('>')
            .unwrap_or_else(|| panic!("unterminated <img> tag: {index}"));
    let tag = &index[tag_start..=tag_end];
    assert!(
        tag.contains("src=\"cover.png\""),
        "the cover img must use the configured path as src: {tag}"
    );
    assert!(
        tag.contains("nolightbox"),
        "the cover img must carry Q1's nolightbox class: {tag}"
    );
    assert!(
        tag.contains("title=\"Minibook\""),
        "the cover img must carry the book title as its title attribute: {tag}"
    );
    // Pinned as observed: q2's HTML writer data-prefixes custom
    // attributes (HTML Living Standard policy in the writer), so Q1's
    // `fig-alt="…"` ships as `data-fig-alt="…"` — a writer-level
    // deviation applying to every custom attribute, not cover-specific.
    assert!(
        tag.contains("data-fig-alt=\"Book cover\""),
        "the cover img must carry the configured alt text: {tag}"
    );

    // Placement: inside the first content <section>, after its header.
    let sec_start = index[..img_pos]
        .rfind("<section")
        .unwrap_or_else(|| panic!("the cover must sit inside a <section>: {index}"));
    assert!(
        index[sec_start..img_pos].contains("<h1"),
        "the cover must come after the section's header (Q1's post-fixup shape): {}",
        &index[sec_start..img_pos]
    );
    assert!(
        index[img_pos..].contains("</section>"),
        "the section holding the cover must close after it: {}",
        &index[img_pos..]
    );

    // And the file itself lands in the output tree so src="cover.png"
    // resolves from the rendered page.
    assert!(
        project_dir.join("_book").join("cover.png").exists(),
        "cover.png must be copied into the book output tree"
    );
}

/// P4 checklist: a cross-chapter `@ref` renders a visible, non-crashing
/// unresolved indicator — the explicit contract P4 ships with (P5's
/// registry replaces it). Already true today via
/// `CrossrefResolveTransform`'s unresolved node; this test pins it so the
/// multi-file wiring can never silently regress it.
#[test]
fn cross_chapter_ref_renders_visible_unresolved_indicator() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);
    let ch2 = render_and_read(&project_dir, "ch2");

    // ch2 references ch1's `@fig-one`: visible `?fig-one?` text, the
    // `quarto-unresolved-ref` marker class, and a dangling in-page
    // target — not a panic, not a silently wrong number.
    assert!(
        ch2.contains("?fig-one?"),
        "the cross-chapter ref must show the literal ?id? placeholder: {}",
        ch2
    );
    assert!(
        ch2.contains("quarto-unresolved-ref"),
        "the cross-chapter ref must carry the unresolved marker class: {}",
        ch2
    );
    assert!(
        ch2.contains("href=\"#fig-one\""),
        "the unresolved ref still carries its (dangling) anchor target: {}",
        ch2
    );
}

/// P4 checklist item — **premise corrected empirically.** The item asked
/// to pin a silent drop of chapter `subtitle`/`date`/`abstract` "for
/// multi-file mode too", assuming P2's merge drop applied equally here.
/// It does not: multi-file HTML renders each chapter with its own front
/// matter, and q2's HTML title-block partial (template.rs) renders all
/// three fields whenever present — **matching Q1**, where a chapter's
/// front-matter subtitle/date/abstract flow into `format.metadata` and
/// render in the chapter page's title block (only `description` is
/// suppressed, via `kHideDescription`). P2's drop is a *merge* artifact —
/// non-first chapters' meta cannot survive the single-file merge — not a
/// render-mode rule. This test pins the real (Q1-faithful) behavior so a
/// future bd-ock6cyiy decision (per-chapter subtitle/date/abstract) starts
/// from the truth: multi-file mode already shows these fields; single-file
/// mode (P2's test) does not.
#[test]
fn chapter_front_matter_subtitle_date_abstract_render_on_chapter_pages() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minibook(&project_dir);
    write(
        &project_dir.join("ch1.qmd"),
        "---\ntitle: \"Chapter One\"\nsubtitle: Chapter One Subtitle\ndate: 2021-02-02\nabstract: Chapter one abstract text.\n---\n\nChapter one's own figure is @fig-one.\n\n```{.python}\n#| label: fig-one\n#| fig-cap: A figure in chapter one\nx = 1\n```\n",
    );

    let ch1 = render_and_read(&project_dir, "ch1");

    assert!(
        ch1.contains("<p class=\"subtitle lead\">Chapter One Subtitle</p>"),
        "chapter front-matter subtitle renders in the title block: {ch1}"
    );
    assert!(
        ch1.contains("<p class=\"date\">February 2, 2021</p>"),
        "chapter front-matter date renders (normalized by DateNormalizeTransform): {ch1}"
    );
    assert!(
        ch1.contains("Chapter one abstract text"),
        "chapter front-matter abstract renders in the title block: {ch1}"
    );
}

/// P4 checklist regression pin: a non-book render — a default project,
/// where no seed map is ever installed (`book_render_items` stays
/// `None`) — shows exactly the pre-P4 output: no H1 number of any kind,
/// flat `Figure 1` caption and ref. The `format_crossref_number` unit
/// tests pin `None`-seed → old bytes at the transform level; this pins
/// the same contract end-to-end through the project pipeline, where a
/// leak would mean the book seed machinery reached a non-book render.
/// (Documenting/pinning — it passed before and after the seed work.)
#[test]
fn non_book_render_numbers_stay_flat() {
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

    let html = render_and_read(&project_dir, "doc");

    assert!(
        !html.contains("header-section-number"),
        "non-book H1 must carry no section number: {}",
        html
    );
    assert!(
        !html.contains("data-number"),
        "non-book H1 must carry no data-number: {}",
        html
    );
    assert!(
        html.contains("Figure\u{a0}1: A solo figure"),
        "non-book caption must stay flat 1: {}",
        html
    );
    assert!(
        html.contains(">Figure\u{a0}1</a>"),
        "non-book ref must stay flat 1: {}",
        html
    );
}

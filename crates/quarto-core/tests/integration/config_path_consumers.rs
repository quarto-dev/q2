/*
 * tests/integration/config_path_consumers.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Declaration-site resolution for the config-path consumers migrated to
 * `quarto_core::config_paths` in Phase 1 (bd-lwvumpfk). Contract:
 * claude-notes/designs/path-resolution-model.md.
 */

//! End-to-end tests, one per migrated consumer, in the shape that
//! distinguishes "resolved against the declaring file" from "resolved
//! against the consuming document": a project-level (or directory-level)
//! declaration consumed by a document in a subdirectory, and the
//! leading-`/` spelling that anchors at the project root.
//!
//! Harness mirrors `tests/integration/filter_paths.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &Path) -> PathBuf {
    quarto_system_runtime::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    write_bytes(path, contents.as_bytes());
}

fn write_bytes(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

fn render_project_as(
    format: &str,
    book: bool,
    fixture: impl FnOnce(&Path),
) -> (PathBuf, ProjectRenderSummary) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::from_format_string(format).unwrap(),
        format,
        &options,
        runtime.clone(),
    );
    let summary = if book {
        pollster::block_on(pipeline.run_with_book_support()).expect("pipeline")
    } else {
        pollster::block_on(pipeline.run()).expect("pipeline")
    };
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary
            .pass1_failures
            .iter()
            .map(|f| (&f.input, &f.error))
            .collect::<Vec<_>>(),
        summary
            .pass2_failures
            .iter()
            .map(|f| (&f.input, &f.error))
            .collect::<Vec<_>>()
    );
    std::mem::forget(temp);
    (project_dir, summary)
}

fn render_project(fixture: impl FnOnce(&Path)) -> (PathBuf, ProjectRenderSummary) {
    render_project_as("html", false, fixture)
}

fn error_diagnostics(summary: &ProjectRenderSummary) -> Vec<String> {
    summary
        .outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter())
        .chain(summary.project_diagnostics.iter())
        .filter(|d| matches!(d.kind, quarto_error_reporting::DiagnosticKind::Error))
        .map(|d| format!("{:?}: {}", d.code, d.title))
        .collect()
}

/// Two-page website: `index.qmd` at the root and `sub/page.qmd`.
/// `config_tail` is appended to `_quarto.yml` verbatim.
fn two_page_site(config_tail: &str) -> impl FnOnce(&Path) + '_ {
    move |project_dir: &Path| {
        write(
            &project_dir.join("_quarto.yml"),
            &format!(
                "project:\n  type: website\n  output-dir: _site\n  render:\n    - index.qmd\n    - sub/page.qmd\n{config_tail}"
            ),
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Root\n---\n\nHello root.\n",
        );
        write(
            &project_dir.join("sub/page.qmd"),
            "---\ntitle: Sub\n---\n\nHello sub.\n",
        );
    }
}

fn files_under(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// template-partials (HTML leg)
// ═══════════════════════════════════════════════════════════════════

const SKIP_LINK_PARTIAL: &str = "<!-- PARTIAL-945-SKIP-LINK -->\n";

#[test]
fn project_template_partials_reach_subdir_pages() {
    let (project_dir, _) = render_project(|p| {
        two_page_site(
            "format:\n  html:\n    template-partials:\n      - partials/skip-link.html\n",
        )(p);
        write(&p.join("partials/skip-link.html"), SKIP_LINK_PARTIAL);
    });
    for page in ["_site/index.html", "_site/sub/page.html"] {
        let html = read(&project_dir.join(page));
        assert!(
            html.contains("PARTIAL-945-SKIP-LINK"),
            "{page}: a project-level template partial must replace the built-in one"
        );
    }
}

#[test]
fn rooted_template_partial_anchors_at_project_root() {
    let (project_dir, _) = render_project(|p| {
        two_page_site(
            "format:\n  html:\n    template-partials:\n      - /partials/skip-link.html\n",
        )(p);
        write(&p.join("partials/skip-link.html"), SKIP_LINK_PARTIAL);
    });
    let html = read(&project_dir.join("_site/sub/page.html"));
    assert!(html.contains("PARTIAL-945-SKIP-LINK"));
}

// ═══════════════════════════════════════════════════════════════════
// shortcodes
// ═══════════════════════════════════════════════════════════════════

#[test]
fn project_shortcodes_reach_subdir_pages() {
    let (project_dir, _) = render_project(|p| {
        two_page_site("shortcodes:\n  - sc.lua\n")(p);
        write(
            &p.join("sc.lua"),
            "function hi(args, kwargs, meta)\n  return pandoc.Str(\"SHORTCODE-945-RAN\")\nend\n",
        );
        write(
            &p.join("sub/page.qmd"),
            "---\ntitle: Sub\n---\n\nSays {{< hi >}}.\n",
        );
    });
    let html = read(&project_dir.join("_site/sub/page.html"));
    assert!(
        html.contains("SHORTCODE-945-RAN"),
        "a project-level shortcode file must resolve for a subdirectory page; got:\n{html}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// language: <file>.yml
// ═══════════════════════════════════════════════════════════════════

#[test]
fn dir_metadata_language_file_resolves_against_its_own_dir() {
    let (project_dir, summary) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n  render:\n    - index.qmd\n    - docs/deep/page.qmd\n",
        );
        write(&p.join("index.qmd"), "---\ntitle: Root\n---\n\nHello.\n");
        write(&p.join("docs/_metadata.yml"), "language: terms.yml\n");
        write(
            &p.join("docs/terms.yml"),
            "toc-title-website: \"CONTENTS-945\"\n",
        );
        write(
            &p.join("docs/deep/page.qmd"),
            "---\ntitle: Deep\ntoc: true\n---\n\n## One\n\nText.\n\n## Two\n\nText.\n",
        );
    });
    let errors = error_diagnostics(&summary);
    assert!(
        errors.is_empty(),
        "no language-file error expected; got {errors:?}"
    );
    let html = read(&project_dir.join("_site/docs/deep/page.html"));
    assert!(
        html.contains("CONTENTS-945"),
        "a `docs/_metadata.yml` language file resolves against `docs/`, not the document dir or the project root; got:\n{html}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// title-block-banner (image probe + copy)
// ═══════════════════════════════════════════════════════════════════

/// A 1x1 RGBA PNG (the smallest valid image).
const TEST_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0,
    0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0xAB, 0xCE, 0x36, 0x89, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

#[test]
fn project_title_banner_image_reaches_subdir_pages() {
    let (project_dir, _) = render_project(|p| {
        two_page_site("title-block-banner: banner.png\n")(p);
        write_bytes(&p.join("banner.png"), TEST_PNG);
    });
    let html = read(&project_dir.join("_site/sub/page.html"));
    assert!(
        html.contains("background-image"),
        "a project-level banner image must be recognised as an image for a subdirectory page \
         (today it is probed in the document's dir, misses, and is treated as a colour); got:\n{html}"
    );
    assert!(
        project_dir.join("_site/sub/banner.png").is_file(),
        "the banner image is copied beside the page that uses it"
    );
}

// ═══════════════════════════════════════════════════════════════════
// revealjs logo (bd-7328beha: URL-space emitter)
// ═══════════════════════════════════════════════════════════════════

/// A website with a reveal deck in a subdirectory; `deck_front_matter`
/// goes into the deck's YAML and `config_tail` into `_quarto.yml`.
fn site_with_deck<'a>(config_tail: &'a str, deck_front_matter: &'a str) -> impl FnOnce(&Path) + 'a {
    move |project_dir: &Path| {
        write(
            &project_dir.join("_quarto.yml"),
            &format!(
                "project:\n  type: website\n  output-dir: _site\n  render:\n    - index.qmd\n    - talks/deck.qmd\n{config_tail}"
            ),
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Root\n---\n\nHello root.\n",
        );
        write(
            &project_dir.join("talks/deck.qmd"),
            &format!(
                "---\ntitle: Deck\nformat: revealjs\n{deck_front_matter}---\n\n## A slide\n\n- one\n"
            ),
        );
    }
}

#[test]
fn project_reveal_logo_reaches_subdir_decks() {
    let (project_dir, _) = render_project(|p| {
        site_with_deck("logo: brand/logo.png\n", "")(p);
        write_bytes(&p.join("brand/logo.png"), TEST_PNG);
    });
    let html = read(&project_dir.join("_site/talks/deck.html"));
    assert!(
        html.contains(r#"<img class="slide-logo" src="../brand/logo.png">"#),
        "a project-level logo is resolved against _quarto.yml and relativized to the deck \
         (today it is emitted verbatim); got:\n{html}"
    );
    assert!(
        project_dir.join("_site/brand/logo.png").is_file(),
        "the logo is copied into the output tree"
    );
}

#[test]
fn rooted_reveal_logo_anchors_at_project_root() {
    let (project_dir, _) = render_project(|p| {
        site_with_deck("", "logo: /brand/logo.png\n")(p);
        write_bytes(&p.join("brand/logo.png"), TEST_PNG);
    });
    let html = read(&project_dir.join("_site/talks/deck.html"));
    assert!(
        html.contains(r#"<img class="slide-logo" src="../brand/logo.png">"#),
        "a leading `/` is the project root, relativized per page; got:\n{html}"
    );
    assert!(project_dir.join("_site/brand/logo.png").is_file());
}

#[test]
fn deck_relative_reveal_logo_resolves_against_the_deck() {
    let (project_dir, _) = render_project(|p| {
        site_with_deck("", "logo: img/logo.png\n")(p);
        write_bytes(&p.join("talks/img/logo.png"), TEST_PNG);
    });
    let html = read(&project_dir.join("_site/talks/deck.html"));
    assert!(
        html.contains(r#"<img class="slide-logo" src="img/logo.png">"#),
        "a front-matter logo is relative to the deck; got:\n{html}"
    );
    assert!(
        project_dir.join("_site/talks/img/logo.png").is_file(),
        "the logo is copied beside the deck"
    );
}

// ═══════════════════════════════════════════════════════════════════
// listing.template (bd-o1meelim: leading `/`)
// ═══════════════════════════════════════════════════════════════════

const CARD_TEMPLATE: &str = "::: {.custom-card-grid}\n\
$for(items)$\n\
::: {.custom-card}\n\
### [$it.title$]($it.path$)\n\
:::\n\
\n\
$endfor$\n\
:::\n";

#[test]
fn rooted_listing_template_anchors_at_project_root() {
    let (project_dir, summary) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(&p.join("templates/cards.template"), CARD_TEMPLATE);
        write(
            &p.join("posts/item-one.qmd"),
            "---\ntitle: First Item\n---\n\nOne.\n",
        );
        write(
            &p.join("posts/index.qmd"),
            "---\ntitle: Cards\nlisting:\n  id: cards\n  type: custom\n  template: /templates/cards.template\n  contents: \"item-*.qmd\"\n---\n\n::: {#cards}\n:::\n",
        );
    });
    let errors = error_diagnostics(&summary);
    let html = read(&project_dir.join("_site/posts/index.html"));
    assert!(
        html.contains("custom-card-grid") && html.contains("First Item"),
        "`template: /templates/cards.template` is project-root-relative (contract rule 2); errors={errors:?}; got:\n{html}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// brand: read from merged metadata
// ═══════════════════════════════════════════════════════════════════

#[test]
fn dir_metadata_brand_resolves_against_its_own_dir() {
    let (project_dir, _) = render_project(|p| {
        two_page_site("")(p);
        write(
            &p.join("sub/_metadata.yml"),
            "brand: brand.yml\nformat:\n  html:\n    theme:\n      - cosmo\n      - brand\n",
        );
        write(
            &p.join("sub/brand.yml"),
            "color:\n  palette:\n    brand-teal: \"#00a0a0\"\n  primary: brand-teal\n",
        );
    });
    let mut css_files = Vec::new();
    files_under(&project_dir.join("_site"), "css", &mut css_files);
    let all_css: String = css_files.iter().map(|f| read(f)).collect();
    assert!(
        all_css.contains("--brand-brand-teal: #00a0a0"),
        "a `brand:` declared in `sub/_metadata.yml` resolves against `sub/`, not the project root; css files: {css_files:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// book.chapters: leading `/`
// ═══════════════════════════════════════════════════════════════════

#[test]
fn rooted_book_chapters_anchor_at_project_root() {
    let (project_dir, summary) = render_project_as("html", true, |p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: book\n  output-dir: _book\nbook:\n  title: Rooted\n  chapters:\n    - /index.qmd\n    - /chapters/intro.qmd\n",
        );
        write(&p.join("index.qmd"), "# Preface\n\nHello.\n");
        write(&p.join("chapters/intro.qmd"), "# Intro\n\nHello intro.\n");
    });
    assert_eq!(summary.outputs.len(), 2, "both chapters render");
    assert!(project_dir.join("_book/chapters/intro.html").is_file());
}

// ═══════════════════════════════════════════════════════════════════
// template-partials (Typst leg)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn project_typst_template_partials_reach_subdir_pages() {
    let (_project_dir, summary) = render_project_as("typst", false, |p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: default\n  render:\n    - sub/page.qmd\nformat:\n  typst:\n    keep-typ: true\n    template-partials:\n      - partials/typst-show.typ\n",
        );
        write(
            &p.join("partials/typst-show.typ"),
            "// Q2-CUSTOM-SHOW-PARTIAL-945\n#show: doc => doc\n",
        );
        write(
            &p.join("sub/page.qmd"),
            "---\ntitle: Sub\n---\n\n# Heading\n\nBody.\n",
        );
    });
    assert_eq!(summary.outputs.len(), 1);
    let typ_path = summary.outputs[0].output_path.with_extension("typ");
    let typ = read(&typ_path);
    assert!(
        typ.contains("Q2-CUSTOM-SHOW-PARTIAL-945"),
        "a project-level typst partial is staged for a subdirectory page; got:\n{typ}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Phase 1b (bd-okqrjh8v): the stored form of a marked path is the
// project-root pivot, the same for every page, and that is what Lua sees
// ═══════════════════════════════════════════════════════════════════

#[test]
fn lua_sees_marked_paths_in_project_root_form() {
    let (project_dir, _) = render_project(|p| {
        two_page_site(
            "format:\n  html:\n    include-in-header:\n      - assets/hdr.html\nfilters:\n  - show.lua\n",
        )(p);
        write(&p.join("assets/hdr.html"), "<!-- hdr -->\n");
        write(
            &p.join("show.lua"),
            "function Pandoc(doc)\n  local inc = doc.meta[\"include-in-header\"]\n  local first = inc[1]\n  local s = type(first) == \"string\" and first or first.value\n  doc.blocks:insert(pandoc.Para(pandoc.Str(\"PIVOT<\" .. s .. \">\")))\n  return doc\nend\n",
        );
    });
    for page in ["_site/index.html", "_site/sub/page.html"] {
        let html = read(&project_dir.join(page));
        assert!(
            html.contains("PIVOT&lt;assets/hdr.html&gt;")
                || html.contains("PIVOT<assets/hdr.html>"),
            "{page}: a marked path is stored project-root-relative, identically for every page \
             (not ../assets/hdr.html for the subdirectory page); got:\n{html}"
        );
    }
}

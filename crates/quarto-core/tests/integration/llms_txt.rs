/*
 * tests/integration/llms_txt.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for `website.llms-txt` support
 * (bd-llms-txt-unimplemented-oih6z6j7): the organized `llms.txt`
 * index, per-page `<page>.md` markdown companions, and
 * `llms-full.txt`.
 *
 * Design: claude-notes/plans/2026-08-14-llms-txt-website-support.md
 *
 * Output contract exercised here (the tests are the spec):
 *
 * - `_site/llms.txt`: `# <site title>`, `> <site description>`
 *   blockquote, then H2 sections derived from the sidebar structure
 *   (single sidebar → one H2 per top-level `section:`; multiple
 *   sidebars → one H2 per sidebar). Entries are
 *   `- [title](href): description` (no trailing `:` when the page
 *   has no description). Pages no sidebar/navbar reaches land in a
 *   final `## Other` section; a site with no navigation at all uses
 *   a single `## Pages` section. The home page is pinned first
 *   (before any section) when navigation doesn't cover it. Hrefs are
 *   companion (`.md`) paths, absolute when `site-url` is set.
 * - `_site/<page>.md`: markdown companion per page (drafts and the
 *   404 page excluded), starting with the `# <title>` heading;
 *   same-site links point at `.md` siblings.
 * - `_site/llms-full.txt`: companions concatenated in index order,
 *   each preceded by a `---\ntitle: <t>\nurl: <href>\n---` header.
 * - Companion writes go through the output ledger: a collision with
 *   any other produced file (e.g. a resource-copied `.md`, or a
 *   user-provided `llms.txt` resource) fails the render with
 *   Q-5-28.
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::error::QuartoError;
use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
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

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Drive a fixture through `ProjectPipeline`, returning the project
/// directory and the pipeline's `Result`.
///
/// Unlike [`render_project`] this does not assert success — the
/// collision tests need the error.
fn try_render_project(
    fixture: impl FnOnce(&Path),
) -> (PathBuf, Result<ProjectRenderSummary, QuartoError>) {
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
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );
    let result = pollster::block_on(pipeline.run());

    // Leak the temp dir so the test can inspect files afterwards
    // (cleanup happens at process exit).
    std::mem::forget(temp);
    (project_dir, result)
}

/// [`try_render_project`] plus the assertion that every page rendered.
fn render_project(fixture: impl FnOnce(&Path)) -> (PathBuf, ProjectRenderSummary) {
    let (project_dir, result) = try_render_project(fixture);
    let summary = result.expect("pipeline should succeed");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    (project_dir, summary)
}

/// Assert `haystack` contains `needle`, with a readable failure.
fn assert_contains(haystack: &str, needle: &str, context: &str) {
    assert!(
        haystack.contains(needle),
        "{context}: expected to find {needle:?} in:\n{haystack}"
    );
}

fn assert_not_contains(haystack: &str, needle: &str, context: &str) {
    assert!(
        !haystack.contains(needle),
        "{context}: expected NOT to find {needle:?} in:\n{haystack}"
    );
}

/// Byte offset of `needle` in `haystack`, for order assertions.
fn pos(haystack: &str, needle: &str, context: &str) -> usize {
    haystack.find(needle).unwrap_or_else(|| {
        panic!("{context}: expected to find {needle:?} in:\n{haystack}");
    })
}

/// Three-page website with a two-section sidebar, descriptions on
/// every page, and `llms-txt: true`. The shared happy-path fixture.
fn sectioned_site(dir: &Path) {
    write(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n\
         website:\n  title: \"Test Site\"\n  description: \"A site for llms tests\"\n\
         \x20 llms-txt: true\n\
         \x20 sidebar:\n    contents:\n\
         \x20     - section: \"Basics\"\n        contents:\n          - index.qmd\n          - about.qmd\n\
         \x20     - section: \"Reference\"\n        contents:\n          - api.qmd\n",
    );
    write(
        &dir.join("index.qmd"),
        "---\ntitle: Home\ndescription: The landing page\n---\n\n\
         Welcome. See [the about page](about.qmd) for details.\n",
    );
    write(
        &dir.join("about.qmd"),
        "---\ntitle: About\ndescription: All about us\n---\n\nAll about our team.\n",
    );
    write(
        &dir.join("api.qmd"),
        "---\ntitle: API\ndescription: API reference\n---\n\nThe API surface.\n",
    );
}

// ═══════════════════════════════════════════════════════════════════
// llms.txt index: structure and organization
// ═══════════════════════════════════════════════════════════════════

/// Single-sidebar site: `# title`, `> description`, one H2 per
/// top-level sidebar section, entries in sidebar order with
/// `: description` annotations, companion (`.md`) hrefs.
#[test]
fn llms_txt_emitted_with_sidebar_sections() {
    let (project_dir, _summary) = render_project(sectioned_site);

    let llms = read(&project_dir.join("_site/llms.txt"));

    assert_contains(&llms, "# Test Site\n", "site title heading");
    assert_contains(
        &llms,
        "> A site for llms tests\n",
        "site description blockquote",
    );
    assert_contains(&llms, "## Basics\n", "first sidebar section");
    assert_contains(&llms, "## Reference\n", "second sidebar section");
    assert_contains(
        &llms,
        "- [Home](index.md): The landing page\n",
        "home entry",
    );
    assert_contains(&llms, "- [About](about.md): All about us\n", "about entry");
    assert_contains(&llms, "- [API](api.md): API reference\n", "api entry");

    // Order: title < description < Basics < Home < About < Reference < API.
    let p_title = pos(&llms, "# Test Site", "order");
    let p_desc = pos(&llms, "> A site", "order");
    let p_basics = pos(&llms, "## Basics", "order");
    let p_home = pos(&llms, "[Home]", "order");
    let p_about = pos(&llms, "[About]", "order");
    let p_ref = pos(&llms, "## Reference", "order");
    let p_api = pos(&llms, "[API]", "order");
    assert!(
        p_title < p_desc
            && p_desc < p_basics
            && p_basics < p_home
            && p_home < p_about
            && p_about < p_ref
            && p_ref < p_api,
        "llms.txt sections out of order:\n{llms}"
    );

    // Index links point at companions, never at HTML outputs.
    assert_not_contains(&llms, ".html", "index links .md companions only");
}

/// A `website.title` / `website.description` written with qmd markup
/// — raw HTML inlines and shortcodes — flattens cleanly into the
/// llms.txt header: raws dropped, shortcodes resolved, formatting
/// reduced to its text (bd-6m1iyxl6). The reference behavior is the
/// browser `<title>`, which already renders this title clean; the
/// index header must use the same resolved data.
#[test]
fn llms_txt_title_flattens_markup_and_resolves_shortcodes() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             sitever: \"9.9\"\n\
             website:\n\
             \x20 title: \"My Site `<small>`{=html}v{{< meta sitever >}}`</small>`{=html}\"\n\
             \x20 description: \"Docs for *My Site*\"\n\
             \x20 llms-txt: true\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
    });

    let llms = read(&project_dir.join("_site/llms.txt"));
    assert_contains(&llms, "# My Site v9.9\n", "clean resolved title");
    assert_contains(
        &llms,
        "> Docs for My Site\n",
        "description formatting flattened",
    );
    assert_not_contains(&llms, "{=html}", "no raw-inline syntax in llms.txt header");
    assert_not_contains(&llms, "<small>", "no raw HTML in llms.txt header");
    assert_not_contains(&llms, "{{<", "no shortcode syntax in llms.txt header");

    // llms-full.txt headers share the same site data path only for
    // page titles; the site title appears nowhere there — but the
    // companions must be equally free of leaked site-title syntax.
    let full = read(&project_dir.join("_site/llms-full.txt"));
    assert_not_contains(
        &full,
        "{=html}",
        "no raw-inline syntax leaks into llms-full",
    );
}

/// With `site-url` set, index hrefs are absolute.
#[test]
fn llms_txt_absolute_urls_with_site_url() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Test Site\"\n  site-url: https://example.com\n\
             \x20 llms-txt: true\n",
        );
        write(
            &dir.join("index.qmd"),
            "---\ntitle: Home\ndescription: The landing page\n---\n\nHi.\n",
        );
        write(&dir.join("about.qmd"), "---\ntitle: About\n---\n\nUs.\n");
    });

    let llms = read(&project_dir.join("_site/llms.txt"));
    assert_contains(
        &llms,
        "- [Home](https://example.com/index.md): The landing page\n",
        "absolute home entry",
    );
    // No description → no trailing colon.
    assert_contains(
        &llms,
        "- [About](https://example.com/about.md)\n",
        "absolute about entry",
    );
}

/// No sidebar and no navbar: all pages in a single `## Pages`
/// section ("Other" is reserved for sites where *some* pages were
/// categorized).
#[test]
fn llms_txt_flat_site_uses_pages_section() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Flat Site\"\n  llms-txt: true\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(&dir.join("about.qmd"), "---\ntitle: About\n---\n\nUs.\n");
    });

    let llms = read(&project_dir.join("_site/llms.txt"));
    assert_contains(&llms, "## Pages\n", "flat fallback section");
    assert_not_contains(&llms, "## Other", "no Other section on a flat site");
    assert_contains(&llms, "- [Home](index.md)\n", "home entry");
    assert_contains(&llms, "- [About](about.md)\n", "about entry");
}

/// Pages the sidebar doesn't reach land in `## Other`; the home
/// page, when uncovered, is pinned before any section instead.
#[test]
fn llms_txt_straggler_in_other_and_home_pinned() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Test Site\"\n  llms-txt: true\n\
             \x20 sidebar:\n    contents:\n\
             \x20     - section: \"Docs\"\n        contents:\n          - about.qmd\n",
        );
        write(
            &dir.join("index.qmd"),
            "---\ntitle: Home\ndescription: The landing page\n---\n\nHi.\n",
        );
        write(&dir.join("about.qmd"), "---\ntitle: About\n---\n\nUs.\n");
        write(&dir.join("extra.qmd"), "---\ntitle: Extra\n---\n\nMore.\n");
    });

    let llms = read(&project_dir.join("_site/llms.txt"));
    assert_contains(
        &llms,
        "- [Home](index.md): The landing page\n",
        "pinned home entry",
    );
    assert_contains(&llms, "## Docs\n", "sidebar section");
    assert_contains(&llms, "## Other\n", "straggler section");
    assert_contains(
        &llms,
        "- [Extra](extra.md)\n",
        "straggler entry, no description",
    );

    // Home before the first section; straggler inside Other.
    let p_home = pos(&llms, "[Home]", "order");
    let p_docs = pos(&llms, "## Docs", "order");
    let p_other = pos(&llms, "## Other", "order");
    let p_extra = pos(&llms, "[Extra]", "order");
    assert!(
        p_home < p_docs && p_docs < p_other && p_other < p_extra,
        "expected home pinned before sections and straggler under Other:\n{llms}"
    );
}

/// Multiple declared sidebars: one H2 per sidebar (using its
/// title), internal sections flattened into the list.
#[test]
fn llms_txt_multi_sidebar_h2_per_sidebar() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Test Site\"\n  llms-txt: true\n\
             \x20 sidebar:\n\
             \x20   - id: guide\n      title: \"Guide\"\n      contents:\n        - index.qmd\n        - about.qmd\n\
             \x20   - id: reference\n      title: \"Reference\"\n      contents:\n        - api.qmd\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(&dir.join("about.qmd"), "---\ntitle: About\n---\n\nUs.\n");
        write(&dir.join("api.qmd"), "---\ntitle: API\n---\n\nRef.\n");
    });

    let llms = read(&project_dir.join("_site/llms.txt"));
    assert_contains(&llms, "## Guide\n", "first sidebar H2");
    assert_contains(&llms, "## Reference\n", "second sidebar H2");
    let p_guide = pos(&llms, "## Guide", "order");
    let p_about = pos(&llms, "[About]", "order");
    let p_ref = pos(&llms, "## Reference", "order");
    let p_api = pos(&llms, "[API]", "order");
    assert!(
        p_guide < p_about && p_about < p_ref && p_ref < p_api,
        "expected sidebar-per-H2 organization:\n{llms}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Per-page markdown companions
// ═══════════════════════════════════════════════════════════════════

/// Every rendered page gets a sibling `<page>.md` whose content is a
/// markdown rendering of the page (title heading + body), with
/// same-site links rewritten to `.md` siblings.
#[test]
fn llms_companion_emitted_per_page() {
    let (project_dir, _summary) = render_project(sectioned_site);

    let about = read(&project_dir.join("_site/about.md"));
    assert_contains(&about, "# About", "companion title heading");
    assert_contains(&about, "All about our team.", "companion body");

    let index = read(&project_dir.join("_site/index.md"));
    assert_contains(&index, "# Home", "companion title heading");
    // The body link `[the about page](about.qmd)` points at the
    // markdown mirror, not the HTML output.
    assert_contains(
        &index,
        "[the about page](about.md)",
        "companion internal link",
    );
    assert_not_contains(&index, "about.html", "no .html links in companions");
    assert_not_contains(&index, "about.qmd", "no source links in companions");

    // Companions are markdown, not HTML.
    assert_not_contains(&about, "<html", "companion is not HTML");
    assert_not_contains(&about, "<div", "companion carries no html chrome");
}

/// Conditional content: `when-format="llms"` shows only in the
/// companion; `unless-format="llms"` (via content-hidden) shows only
/// in the HTML.
#[test]
fn llms_conditional_content_when_format_llms() {
    let (project_dir, summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Test Site\"\n  llms-txt: true\n",
        );
        write(
            &dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nAlways here.\n\n\
             ::: {.content-visible when-format=\"llms\"}\nLLMSONLYTEXT\n:::\n\n\
             ::: {.content-hidden when-format=\"llms\"}\nHUMANONLYTEXT\n:::\n",
        );
    });

    let html = {
        let path = summary
            .outputs
            .iter()
            .find(|o| o.output_path.file_name().and_then(|s| s.to_str()) == Some("index.html"))
            .expect("index.html output")
            .output_path
            .clone();
        read(&path)
    };
    assert_contains(&html, "Always here.", "html body");
    assert_contains(
        &html,
        "HUMANONLYTEXT",
        "html keeps content hidden from llms",
    );
    assert_not_contains(&html, "LLMSONLYTEXT", "html drops llms-only content");
    // The marker classes are pipeline-internal; neither may survive
    // into the HTML writer.
    assert_not_contains(&html, "quarto-llms", "marker classes never reach the HTML");

    let md = read(&project_dir.join("_site/index.md"));
    assert_contains(&md, "Always here.", "companion body");
    assert_contains(&md, "LLMSONLYTEXT", "companion keeps llms-only content");
    assert_not_contains(&md, "HUMANONLYTEXT", "companion drops llms-hidden content");
}

// ═══════════════════════════════════════════════════════════════════
// llms-full.txt
// ═══════════════════════════════════════════════════════════════════

/// Companions concatenated in index order, each preceded by a
/// `---` header block carrying title + href.
#[test]
fn llms_full_txt_emitted() {
    let (project_dir, _summary) = render_project(sectioned_site);

    let full = read(&project_dir.join("_site/llms-full.txt"));
    assert_contains(
        &full,
        "---\ntitle: Home\nurl: index.md\n---\n",
        "home separator",
    );
    assert_contains(
        &full,
        "---\ntitle: About\nurl: about.md\n---\n",
        "about separator",
    );
    assert_contains(
        &full,
        "---\ntitle: API\nurl: api.md\n---\n",
        "api separator",
    );
    assert_contains(&full, "All about our team.", "about body present");
    assert_contains(&full, "The API surface.", "api body present");

    // Index order: Home (Basics) < About (Basics) < API (Reference).
    let p_home = pos(&full, "title: Home", "order");
    let p_about = pos(&full, "title: About", "order");
    let p_api = pos(&full, "title: API", "order");
    assert!(
        p_home < p_about && p_about < p_api,
        "llms-full.txt pages out of index order:\n{full}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Exclusions and gating
// ═══════════════════════════════════════════════════════════════════

/// Without `llms-txt: true`, none of the artifacts appear.
#[test]
fn llms_artifacts_absent_by_default() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Test Site\"\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
    });

    assert!(
        !project_dir.join("_site/llms.txt").exists(),
        "no llms.txt by default"
    );
    assert!(
        !project_dir.join("_site/llms-full.txt").exists(),
        "no llms-full.txt by default"
    );
    assert!(
        !project_dir.join("_site/index.md").exists(),
        "no companion by default"
    );
}

/// `llms-txt: false` behaves like absent.
#[test]
fn llms_artifacts_absent_when_disabled() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Test Site\"\n  llms-txt: false\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
    });

    assert!(
        !project_dir.join("_site/llms.txt").exists(),
        "no llms.txt when disabled"
    );
    assert!(
        !project_dir.join("_site/index.md").exists(),
        "no companion when disabled"
    );
}

/// Draft pages get no companion and no index entry (they still
/// render to HTML).
#[test]
fn llms_draft_page_excluded() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Test Site\"\n  llms-txt: true\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(
            &dir.join("secret.qmd"),
            "---\ntitle: Secret\ndraft: true\n---\n\nNot yet.\n",
        );
    });

    assert!(
        project_dir.join("_site/secret.html").exists(),
        "draft still renders to HTML"
    );
    assert!(
        !project_dir.join("_site/secret.md").exists(),
        "draft gets no companion"
    );
    let llms = read(&project_dir.join("_site/llms.txt"));
    assert_not_contains(&llms, "secret", "draft absent from llms.txt");

    let full = read(&project_dir.join("_site/llms-full.txt"));
    assert_not_contains(&full, "Not yet.", "draft absent from llms-full.txt");
}

/// The 404 page is excluded from companions and the index.
#[test]
fn llms_404_page_excluded() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Test Site\"\n  llms-txt: true\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(
            &dir.join("404.qmd"),
            "---\ntitle: Not Found\n---\n\nNo such page.\n",
        );
    });

    assert!(
        !project_dir.join("_site/404.md").exists(),
        "404 page gets no companion"
    );
    let llms = read(&project_dir.join("_site/llms.txt"));
    assert_not_contains(&llms, "404", "404 absent from llms.txt");
    assert_not_contains(&llms, "Not Found", "404 title absent from llms.txt");
}

/// On a non-website project, `llms-txt: true` warns that the key is
/// inert and produces no artifacts.
#[test]
fn llms_txt_inert_on_default_project_warns() {
    let (project_dir, summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: default\n  output-dir: _out\n\
             website:\n  llms-txt: true\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
    });

    assert!(
        !project_dir.join("_out/llms.txt").exists(),
        "no llms.txt on a default project"
    );
    assert!(
        summary
            .project_diagnostics
            .iter()
            .any(|d| d.title.contains("llms-txt")),
        "expected an inert-key warning mentioning llms-txt; got: {:?}",
        summary
            .project_diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════
// Output-ledger collisions (Q-5-28)
// ═══════════════════════════════════════════════════════════════════

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn expect_error_code(result: Result<ProjectRenderSummary, QuartoError>, code: &str) -> String {
    let err = match result {
        Ok(_) => panic!("expected the render to fail with {code}, but it succeeded"),
        Err(e) => e,
    };
    let QuartoError::Parse(parse) = &err else {
        panic!("expected QuartoError::Parse carrying diagnostics, got: {err:?}");
    };
    let codes: Vec<String> = parse
        .diagnostics
        .iter()
        .map(|d| d.code.clone().unwrap_or_default())
        .collect();
    assert!(
        codes.iter().any(|c| c == code),
        "expected a diagnostic with code {code}; got codes {codes:?} \
         (titles: {:?})",
        parse
            .diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
    );
    strip_ansi(&parse.render())
}

/// A resource-copied `.md` file at a companion path fails the
/// render: `about.qmd`'s companion wants `_site/about.md`, but the
/// user's `about.md` resource is copied there.
#[test]
fn llms_companion_collision_with_copied_resource_fails() {
    let (_dir, result) = try_render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             \x20 resources:\n    - about.md\n\
             website:\n  title: \"Test Site\"\n  llms-txt: true\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(&dir.join("about.qmd"), "---\ntitle: About\n---\n\nUs.\n");
        write(&dir.join("about.md"), "A user resource, not a companion.\n");
    });

    let rendered = expect_error_code(result, "Q-5-28");
    assert!(
        rendered.contains("about.md"),
        "diagnostic should name the contested path; got:\n{rendered}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Serialization quality (snapshot)
// ═══════════════════════════════════════════════════════════════════

/// Companion serialization of a content-rich page: headings with
/// anchors, callouts, code blocks, footnotes, tables with crossrefs,
/// resolved `@ref` text, inline formatting. Snapshot-reviewed so
/// quality regressions are visible in the diff.
/// A listing page's companion replaces the rendered listing DOM
/// (thumbnail/metadata div chrome, L7 placeholder envelopes) with a
/// clean markdown list synthesized from the resolved listing items:
/// `- [title](href) (date, author): description` (bd-5w81o2dh). The
/// HTML keeps its full listing DOM.
#[test]
fn llms_listing_page_companion_synthesizes_item_list() {
    let (project_dir, summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Test Site\"\n  llms-txt: true\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(
            &dir.join("posts/index.qmd"),
            "---\ntitle: Blog\nlisting: default\n---\n\nRecent posts.\n",
        );
        write(
            &dir.join("posts/a.qmd"),
            "---\ntitle: First\ndate: 2026-01-15\nauthor: Alice\ndescription: First desc.\n---\n\nFirst body.\n",
        );
        write(
            &dir.join("posts/b.qmd"),
            "---\ntitle: Second\ndate: 2026-02-20\nauthor: Bob\n---\n\nSecond body.\n",
        );
    });

    let md = read(&project_dir.join("_site/posts/index.md"));
    assert_contains(&md, "# Blog", "listing page title");
    assert_contains(&md, "Recent posts.", "page prose kept");
    // Synthesized entries: title link (companion href, page-relative),
    // date + author parenthetical, description when present.
    assert_contains(
        &md,
        "* [First](a.md) (2026-01-15, Alice): First desc.\n",
        "first item entry",
    );
    assert_contains(
        &md,
        "* [Second](b.md) (2026-02-20, Bob)\n",
        "second item entry, no description",
    );
    let p_first = pos(&md, "[First]", "order");
    let p_second = pos(&md, "[Second]", "order");
    assert!(p_first < p_second, "items in listing order:\n{md}");

    // None of the rendered listing DOM leaks into the companion.
    assert_not_contains(&md, "thumbnail", "no thumbnail chrome");
    assert_not_contains(&md, "listing-title", "no listing-title chrome");
    assert_not_contains(&md, "listing-description", "no description chrome");
    assert_not_contains(&md, "no-external", "no link-class chrome");
    assert_not_contains(&md, ".metadata", "no metadata chrome");

    // The HTML page keeps the real rendered listing.
    let html = {
        let path = summary
            .outputs
            .iter()
            .find(|o| {
                o.output_path.ends_with("posts/index.html")
                    || o.output_path.ends_with("posts\\index.html")
            })
            .expect("posts/index.html output")
            .output_path
            .clone();
        read(&path)
    };
    assert_contains(&html, "data-listing-rendered", "html keeps the listing DOM");
}

#[test]
fn llms_companion_rich_content_snapshot() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Test Site\"\n  llms-txt: true\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(
            &dir.join("rich.qmd"),
            "---\ntitle: Rich Page\n---\n\n\
             ## Getting started\n\n\
             Some *emphasis*, **strong**, and `inline code`.[^note]\n\n\
             [^note]: A footnote body.\n\n\
             ::: {.callout-note}\nCallouts survive as fenced divs.\n:::\n\n\
             ```python\nprint(\"hello\")\n```\n\n\
             | a | b |\n|---|---|\n| 1 | 2 |\n\n\
             : Numbers {#tbl-nums}\n\n\
             See @tbl-nums for details.\n",
        );
    });

    let md = read(&project_dir.join("_site/rich.md"));
    insta::assert_snapshot!("llms_companion_rich_content", md);
}

// ═══════════════════════════════════════════════════════════════════
// Incremental renders
// ═══════════════════════════════════════════════════════════════════

/// Subset (Mode B) re-render: the targeted page's companion is
/// rewritten; the untouched page's companion survives from the
/// previous run; llms.txt and llms-full.txt still cover both pages
/// (the skipped page's content read back from disk).
#[test]
fn llms_incremental_render_covers_skipped_pages() {
    use quarto_core::project::orchestrator::RenderMode;

    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n\
         website:\n  title: \"Test Site\"\n  llms-txt: true\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\ndescription: The landing page\n---\n\nOriginal home body.\n",
    );
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\ndescription: All about us\n---\n\nAbout body.\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

    // Cold full render.
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );
    pollster::block_on(pipeline.run()).expect("cold render");
    assert_contains(
        &read(&project_dir.join("_site/index.md")),
        "Original home body.",
        "cold companion",
    );

    // Edit the home page; re-render only that page (Mode B).
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\ndescription: The landing page\n---\n\nUpdated home body.\n",
    );
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let targets: std::collections::HashSet<PathBuf> =
        [project_dir.join("index.qmd")].into_iter().collect();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    )
    .with_mode(RenderMode::Subset(targets));
    pollster::block_on(pipeline.run()).expect("incremental render");

    let index_md = read(&project_dir.join("_site/index.md"));
    assert_contains(&index_md, "Updated home body.", "companion refreshed");

    let about_md = read(&project_dir.join("_site/about.md"));
    assert_contains(&about_md, "About body.", "skipped companion survives");

    let llms = read(&project_dir.join("_site/llms.txt"));
    assert_contains(
        &llms,
        "- [Home](index.md): The landing page\n",
        "home entry",
    );
    assert_contains(&llms, "- [About](about.md): All about us\n", "about entry");

    let full = read(&project_dir.join("_site/llms-full.txt"));
    assert_contains(&full, "Updated home body.", "full has refreshed page");
    assert_contains(
        &full,
        "About body.",
        "full covers the skipped page via its on-disk companion",
    );

    std::mem::forget(temp);
}

/// A user-provided `llms.txt` resource collides with the generated
/// index.
#[test]
fn llms_user_provided_llms_txt_collision_fails() {
    let (_dir, result) = try_render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             \x20 resources:\n    - llms.txt\n\
             website:\n  title: \"Test Site\"\n  llms-txt: true\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(&dir.join("llms.txt"), "my own hand-written llms.txt\n");
    });

    let rendered = expect_error_code(result, "Q-5-28");
    assert!(
        rendered.contains("llms.txt"),
        "diagnostic should name the contested path; got:\n{rendered}"
    );
}

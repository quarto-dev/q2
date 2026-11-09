/*
 * tests/integration/website_drafts.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for website drafts and `draft-mode`
 * (bd-2hy80x1j).
 *
 * Plan: claude-notes/plans/2026-10-21-draft-mode-gone.md
 *
 * `draft-mode` is ordinary merged metadata: it may be written as
 * `draft-mode` or `website.draft-mode`, in `_quarto.yml`, a
 * directory's `_metadata.yml`, or a page's front matter, and each
 * draft page is governed by the value its own merged metadata
 * resolves to. These tests observe linkability through the
 * llms.txt markdown companions — a draft that is linkable (effective
 * mode `visible`) gets a companion; any other draft does not.
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::drafts::RenderAudience;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &Path) -> PathBuf {
    quarto_system_runtime::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn render_project(fixture: impl FnOnce(&Path)) -> (PathBuf, ProjectRenderSummary) {
    render_project_with(RenderToFileOptions::default(), fixture)
}

fn render_project_with(
    options: RenderToFileOptions,
    fixture: impl FnOnce(&Path),
) -> (PathBuf, ProjectRenderSummary) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("pipeline should succeed");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    // Leak the temp dir so the test can inspect files afterwards
    // (cleanup happens at process exit).
    std::mem::forget(temp);
    (project_dir, summary)
}

/// `_quarto.yml` for a website with llms companions on, plus `extra`
/// (already indented YAML lines appended at top level).
fn quarto_yml(extra: &str) -> String {
    format!(
        "project:\n  type: website\n  output-dir: _site\n\
         website:\n  title: \"Drafts\"\n  llms-txt: true\n{extra}"
    )
}

fn draft_page(title: &str, extra_front_matter: &str) -> String {
    format!("---\ntitle: {title}\ndraft: true\n{extra_front_matter}---\n\nDraft body.\n")
}

fn has_companion(project_dir: &Path, md: &str) -> bool {
    project_dir.join("_site").join(md).exists()
}

fn diagnostic_codes(summary: &ProjectRenderSummary) -> Vec<String> {
    summary
        .outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter())
        .chain(summary.project_diagnostics.iter())
        .filter_map(|d| d.code.clone())
        .collect()
}

// ═══════════════════════════════════════════════════════════════════
// Where `draft-mode` may be written
// ═══════════════════════════════════════════════════════════════════

/// Absent `draft-mode` means `gone`: the draft is not linkable.
#[test]
fn default_mode_hides_drafts() {
    let (dir, _) = render_project(|d| {
        write(&d.join("_quarto.yml"), &quarto_yml(""));
        write(&d.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(&d.join("secret.qmd"), &draft_page("Secret", ""));
    });
    assert!(has_companion(&dir, "index.md"));
    assert!(!has_companion(&dir, "secret.md"));
}

/// `website.draft-mode` in `_quarto.yml` — Quarto 1's spelling.
#[test]
fn website_spelling_in_quarto_yml() {
    let (dir, _) = render_project(|d| {
        write(
            &d.join("_quarto.yml"),
            &quarto_yml("  draft-mode: visible\n"),
        );
        write(&d.join("secret.qmd"), &draft_page("Secret", ""));
    });
    assert!(has_companion(&dir, "secret.md"));
}

/// Top-level `draft-mode` in `_quarto.yml` is the same as every page
/// declaring it.
#[test]
fn top_level_spelling_in_quarto_yml() {
    let (dir, _) = render_project(|d| {
        write(
            &d.join("_quarto.yml"),
            &format!("{}draft-mode: visible\n", quarto_yml("")),
        );
        write(&d.join("secret.qmd"), &draft_page("Secret", ""));
    });
    assert!(has_companion(&dir, "secret.md"));
}

/// A directory's `_metadata.yml` scopes `draft-mode` to that
/// directory; drafts elsewhere keep the project default.
#[test]
fn metadata_yml_scopes_draft_mode_to_its_directory() {
    let (dir, _) = render_project(|d| {
        write(&d.join("_quarto.yml"), &quarto_yml(""));
        write(&d.join("posts/_metadata.yml"), "draft-mode: visible\n");
        write(&d.join("posts/wip.qmd"), &draft_page("WIP", ""));
        write(&d.join("notes/wip.qmd"), &draft_page("Other WIP", ""));
    });
    assert!(
        has_companion(&dir, "posts/wip.md"),
        "posts/ draft is visible"
    );
    assert!(
        !has_companion(&dir, "notes/wip.md"),
        "other drafts stay gone"
    );
}

/// Front matter overrides the project value for one page, in either
/// direction.
#[test]
fn front_matter_overrides_project_value() {
    let (dir, _) = render_project(|d| {
        write(
            &d.join("_quarto.yml"),
            &quarto_yml("  draft-mode: visible\n"),
        );
        write(
            &d.join("hidden.qmd"),
            &draft_page("Hidden", "website:\n  draft-mode: unlinked\n"),
        );
        write(&d.join("shown.qmd"), &draft_page("Shown", ""));
    });
    assert!(!has_companion(&dir, "hidden.md"));
    assert!(has_companion(&dir, "shown.md"));
}

// ═══════════════════════════════════════════════════════════════════
// Invalid values
// ═══════════════════════════════════════════════════════════════════

/// An unknown value warns (Q-5-49) on each draft page and falls back
/// to `gone`; non-draft pages do not warn.
#[test]
fn invalid_value_warns_on_draft_pages_and_falls_back_to_gone() {
    let (dir, summary) = render_project(|d| {
        write(
            &d.join("_quarto.yml"),
            &quarto_yml("  draft-mode: hidden\n"),
        );
        write(&d.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(&d.join("secret.qmd"), &draft_page("Secret", ""));
    });
    assert!(!has_companion(&dir, "secret.md"));
    let codes = diagnostic_codes(&summary);
    assert_eq!(
        codes.iter().filter(|c| *c == "Q-5-49").count(),
        1,
        "one Q-5-49, from the one draft page; got {codes:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Authoring views
// ═══════════════════════════════════════════════════════════════════

/// An authoring view (`q2 preview --static`, the preview server's
/// native routes) shows every draft, even under an explicit
/// `draft-mode: gone` — the mode governs reading views only.
#[test]
fn authoring_view_overrides_explicit_gone() {
    let fixture = |d: &Path| {
        write(&d.join("_quarto.yml"), &quarto_yml("  draft-mode: gone\n"));
        write(&d.join("secret.qmd"), &draft_page("Secret", ""));
    };
    let (dir, _) = render_project(fixture);
    assert!(!has_companion(&dir, "secret.md"), "reading view hides it");

    let options = RenderToFileOptions {
        audience: RenderAudience::Authoring,
        ..Default::default()
    };
    let (dir, _) = render_project_with(options, fixture);
    assert!(has_companion(&dir, "secret.md"), "authoring view shows it");
}

// ═══════════════════════════════════════════════════════════════════
// The draft page itself
// ═══════════════════════════════════════════════════════════════════

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Under `gone` (the default) the draft's URL still exists, but the page
/// is a fixed blank shell: no title, no body, no banner, no navigation,
/// marked `noindex` and `quarto:status=draft-remove`.
#[test]
fn gone_draft_is_a_blank_noindex_page() {
    let (dir, _) = render_project(|d| {
        write(
            &d.join("_quarto.yml"),
            &quarto_yml("  navbar:\n    left:\n      - index.qmd\n"),
        );
        write(&d.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(&d.join("secret.qmd"), &draft_page("Secret Title", ""));
    });
    let html = read(&dir.join("_site/secret.html"));
    assert!(
        html.contains(r#"<meta name="robots" content="noindex">"#),
        "{html}"
    );
    assert!(html.contains(r#"content="draft-remove""#), "{html}");
    assert!(!html.contains("Draft body."), "body must be gone: {html}");
    assert!(!html.contains("Secret Title"), "title must be gone: {html}");
    assert!(!html.contains("quarto-draft-alert"), "no banner: {html}");
    assert!(!html.contains("navbar"), "no site chrome: {html}");
    assert!(!html.contains("<script"), "no scripts: {html}");

    // Non-draft pages are untouched.
    let home = read(&dir.join("_site/index.html"));
    assert!(home.contains("Hi."));
}

/// Under `unlinked` the draft renders fully, with the banner.
#[test]
fn unlinked_draft_renders_with_banner() {
    let (dir, _) = render_project(|d| {
        write(
            &d.join("_quarto.yml"),
            &quarto_yml("  draft-mode: unlinked\n"),
        );
        write(&d.join("secret.qmd"), &draft_page("Secret Title", ""));
    });
    let html = read(&dir.join("_site/secret.html"));
    assert!(html.contains("Draft body."), "{html}");
    assert!(html.contains("quarto-draft-alert"), "{html}");
    assert!(!html.contains("draft-remove"), "{html}");
}

/// An authoring view never blanks a draft.
#[test]
fn authoring_view_never_blanks() {
    let options = RenderToFileOptions {
        audience: RenderAudience::Authoring,
        ..Default::default()
    };
    let (dir, _) = render_project_with(options, |d| {
        write(&d.join("_quarto.yml"), &quarto_yml(""));
        write(&d.join("secret.qmd"), &draft_page("Secret Title", ""));
    });
    let html = read(&dir.join("_site/secret.html"));
    assert!(html.contains("Draft body."), "{html}");
    assert!(html.contains("quarto-draft-alert"), "{html}");
}

/// Blanking is a website behaviour: in other project types `gone`
/// behaves as `unlinked`, so drafts keep rendering with their banner.
#[test]
fn non_website_project_never_blanks() {
    let (dir, _) = render_project(|d| {
        write(
            &d.join("_quarto.yml"),
            "project:\n  type: default\ndraft-mode: gone\n",
        );
        write(&d.join("secret.qmd"), &draft_page("Secret Title", ""));
    });
    let html = read(&dir.join("secret.html"));
    assert!(html.contains("Draft body."), "{html}");
    assert!(html.contains("quarto-draft-alert"), "{html}");
}

// ═══════════════════════════════════════════════════════════════════
// Unlinking
// ═══════════════════════════════════════════════════════════════════

/// The quarto-web example: a navbar listing two pages, one a draft,
/// plus a sidebar, a footer, a body link, a listing and a sitemap, all
/// pointing at the draft. `extra` sets `draft-mode` (or nothing).
fn linked_draft_site(d: &Path, draft_mode_line: &str) {
    write(
        &d.join("_quarto.yml"),
        &format!(
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Cool Website\"\n  site-url: https://example.com\n\
             {draft_mode_line}\
             \x20 navbar:\n    left:\n      - stuff/item1.qmd\n      - stuff/item2.qmd\n\
             \x20 sidebar:\n    contents:\n      - stuff/item1.qmd\n      - stuff/item2.qmd\n\
             \x20 page-footer:\n    left:\n      - stuff/item1.qmd\n      - stuff/item2.qmd\n"
        ),
    );
    write(
        &d.join("index.qmd"),
        "---\ntitle: Home\nlisting:\n  id: all\n  contents: \"stuff/*.qmd\"\n---\n\n\
         See [the second item](stuff/item2.qmd) and [the first](stuff/item1.qmd).\n\n\
         ::: {#all}\n:::\n",
    );
    write(
        &d.join("stuff/item1.qmd"),
        "---\ntitle: Item 1\n---\n\nFirst.\n",
    );
    write(
        &d.join("stuff/item2.qmd"),
        "---\ntitle: Item 2\ndraft: true\n---\n\nSecond.\n",
    );
}

/// Count `href="…stuff/item2.html"` occurrences.
fn links_to_item2(html: &str) -> usize {
    html.matches("item2.html\"").count()
}

fn assert_item2_unlinked(dir: &Path) {
    let home = read(&dir.join("_site/index.html"));
    assert_eq!(
        links_to_item2(&home),
        0,
        "no navbar, sidebar, footer, body or listing link to the draft:\n{home}"
    );
    assert!(
        home.contains("the second item"),
        "the body link's text stays when the link goes"
    );
    assert!(links_to_item2(&read(&dir.join("_site/stuff/item1.html"))) == 0);
    let sitemap = read(&dir.join("_site/sitemap.xml"));
    assert!(!sitemap.contains("item2"), "{sitemap}");
    assert!(sitemap.contains("item1"), "{sitemap}");
}

#[test]
fn gone_drafts_are_unlinked_everywhere() {
    let (dir, summary) = render_project(|d| linked_draft_site(d, ""));
    assert_item2_unlinked(&dir);
    assert!(!diagnostic_codes(&summary).iter().any(|c| c == "Q-12-19"));
}

/// A listing glob whose only matches are hidden drafts did match
/// something: it lists nothing, but must not warn Q-12-19 "matched
/// nothing".
#[test]
fn listing_glob_matching_only_drafts_does_not_warn() {
    let (dir, summary) = render_project(|d| {
        write(&d.join("_quarto.yml"), &quarto_yml(""));
        write(
            &d.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  contents: \"posts/*.qmd\"\n---\n",
        );
        write(&d.join("posts/wip.qmd"), &draft_page("WIP", ""));
    });
    let codes = diagnostic_codes(&summary);
    assert!(!codes.iter().any(|c| c == "Q-12-19"), "{codes:?}");
    assert!(!read(&dir.join("_site/index.html")).contains("wip.html"));
}

#[test]
fn unlinked_drafts_are_unlinked_everywhere_but_render() {
    let (dir, _) = render_project(|d| linked_draft_site(d, "  draft-mode: unlinked\n"));
    assert_item2_unlinked(&dir);
    let item2 = read(&dir.join("_site/stuff/item2.html"));
    assert!(item2.contains("Second."), "{item2}");
    assert!(item2.contains("quarto-draft-alert"), "{item2}");
}

#[test]
fn visible_drafts_stay_linked() {
    let (dir, _) = render_project(|d| linked_draft_site(d, "  draft-mode: visible\n"));
    let home = read(&dir.join("_site/index.html"));
    // navbar + sidebar + footer + body link + listing item.
    assert!(
        links_to_item2(&home) >= 5,
        "every surface links the visible draft:\n{home}"
    );
    assert!(read(&dir.join("_site/sitemap.xml")).contains("item2"));
}

/// A page under the authoring audience sees the draft linked, even with
/// the default `gone`.
#[test]
fn authoring_view_keeps_draft_links() {
    let options = RenderToFileOptions {
        audience: RenderAudience::Authoring,
        ..Default::default()
    };
    let (dir, _) = render_project_with(options, |d| linked_draft_site(d, ""));
    let home = read(&dir.join("_site/index.html"));
    assert!(links_to_item2(&home) >= 5, "{home}");
}

// ═══════════════════════════════════════════════════════════════════
// `drafts:` — marking pages by path
// ═══════════════════════════════════════════════════════════════════

fn plain_page(title: &str) -> String {
    format!("---\ntitle: {title}\n---\n\nPublished body.\n")
}

/// A page listed in `website.drafts` is a draft (here: `gone`, so a
/// blank page and no companion), without saying `draft: true` itself.
#[test]
fn website_drafts_list_marks_pages() {
    let (dir, _) = render_project(|d| {
        write(
            &d.join("_quarto.yml"),
            &quarto_yml("  drafts:\n    - posts/wip.qmd\n"),
        );
        write(&d.join("posts/wip.qmd"), &plain_page("WIP"));
        write(&d.join("posts/done.qmd"), &plain_page("Done"));
    });
    assert!(!has_companion(&dir, "posts/wip.md"));
    assert!(has_companion(&dir, "posts/done.md"));
    let wip = read(&dir.join("_site/posts/wip.html"));
    assert!(wip.contains("draft-remove"), "{wip}");
}

/// Globs and bare directories work; the top-level `drafts` spelling is
/// accepted too.
#[test]
fn drafts_list_takes_globs_and_directories() {
    let (dir, _) = render_project(|d| {
        write(
            &d.join("_quarto.yml"),
            &format!(
                "{}drafts:\n  - \"posts/wip-*.qmd\"\n  - scratch\n",
                quarto_yml("")
            ),
        );
        write(&d.join("posts/wip-one.qmd"), &plain_page("One"));
        write(&d.join("posts/final.qmd"), &plain_page("Final"));
        write(&d.join("scratch/deep/idea.qmd"), &plain_page("Idea"));
    });
    assert!(!has_companion(&dir, "posts/wip-one.md"));
    assert!(has_companion(&dir, "posts/final.md"));
    assert!(!has_companion(&dir, "scratch/deep/idea.md"));
}

/// A pattern in `posts/_metadata.yml` resolves relative to `posts/`
/// and reaches only pages under it.
#[test]
fn drafts_list_in_metadata_yml_is_relative_to_its_directory() {
    let (dir, _) = render_project(|d| {
        write(&d.join("_quarto.yml"), &quarto_yml(""));
        write(&d.join("posts/_metadata.yml"), "drafts:\n  - wip.qmd\n");
        write(&d.join("posts/wip.qmd"), &plain_page("Posts WIP"));
        write(&d.join("wip.qmd"), &plain_page("Root WIP"));
    });
    assert!(!has_companion(&dir, "posts/wip.md"));
    assert!(has_companion(&dir, "wip.md"));
}

/// The listed page gets `draft-mode` like any other draft: under
/// `unlinked` it renders with the banner.
#[test]
fn drafts_list_pages_follow_draft_mode() {
    let (dir, _) = render_project(|d| {
        write(
            &d.join("_quarto.yml"),
            &quarto_yml("  draft-mode: unlinked\n  drafts:\n    - wip.qmd\n"),
        );
        write(&d.join("wip.qmd"), &plain_page("WIP"));
    });
    let wip = read(&dir.join("_site/wip.html"));
    assert!(wip.contains("Published body."), "{wip}");
    assert!(wip.contains("quarto-draft-alert"), "{wip}");
}

/// A pattern escaping the project warns Q-5-50 and marks nothing.
#[test]
fn escaping_drafts_pattern_warns() {
    let (dir, summary) = render_project(|d| {
        write(
            &d.join("_quarto.yml"),
            &quarto_yml("  drafts:\n    - ../outside.qmd\n"),
        );
        write(&d.join("index.qmd"), &plain_page("Home"));
    });
    assert!(has_companion(&dir, "index.md"));
    let codes = diagnostic_codes(&summary);
    assert!(codes.iter().any(|c| c == "Q-5-50"), "{codes:?}");
}

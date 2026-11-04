/*
 * tests/integration/website_search.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for website search (bd-eip0naf4): the Pagefind
 * bundle, the bundle manifest, the Quarto 1-shaped `search.json`,
 * and the per-page wiring (options JSON, offset meta, search box).
 *
 * Design: claude-notes/plans/2026-10-14-website-search.md
 */

//! Output contract exercised here (the tests are the spec):
//!
//! - Search is **on by default** for website projects, as in Quarto 1;
//!   `website.search: false` turns everything off (no bundle, no
//!   `search.json`, no page wiring).
//! - `_site/search.json` is written only when wanted: by default when
//!   `website.search.algolia` is fully configured, or as
//!   `website.search.search-json: true | false` says (bd-cdehspam).
//!   When it stops being wanted, a file q2 wrote and nobody changed
//!   since is removed; any other `search.json` is left alone.
//! - `_site/site_libs/quarto-search/pagefind/`: the Pagefind bundle
//!   (`pagefind-entry.json`, `pagefind.js`, `wasm.<lang>.pagefind`,
//!   `*.pf_meta`, `index/*.pf_index`, `fragment/*.pf_fragment`).
//! - `_site/site_libs/quarto-search/manifest.json`: what a federated
//!   parent reads before merging: `version` (manifest format), `quarto`,
//!   `pagefind` (index format / runtime version), `title`, `languages`,
//!   `bundle` (relative to the manifest), `pages`.
//! - `_site/search.json`: Quarto 1's record array, one record per page
//!   (`section: ""`, text before the first level-2 section) plus one per
//!   level-2 section (`href#id`, `section` = heading text, h3+ folded
//!   in), `objectID == href`, `crumbs` from the sidebar trail with the
//!   navbar crumb merged in front (`merge-navbar-crumbs`).
//! - Drafts, pages with `search: false`, and `404.html` are excluded
//!   from the bundle and from `search.json`.
//! - A `RenderMode::Subset` render keeps the pages it skipped in the
//!   index (per-page capture cache under `.quarto/search/`).
//! - Each page carries `<script id="quarto-search-options"
//!   type="application/json">` with Quarto 1's keys plus `bundle` and
//!   `federate`, and `<meta name="quarto:offset" content="…">` giving
//!   the page's path back to the site root.
//! - With a navbar the search box lives in the navbar as an overlay
//!   (`#quarto-search` inside `nav.navbar`); without one, in the
//!   sidebar as a textbox.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::error::QuartoError;
use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{
    ProjectPipeline, ProjectRenderSummary, RenderMode, project_type_for,
};
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

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&read(path)).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e))
}

fn render_in(project_dir: &Path, mode: RenderMode) -> Result<ProjectRenderSummary, QuartoError> {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    )
    .with_mode(mode);
    let mut pipeline = pipeline;
    pollster::block_on(pipeline.run())
}

fn assert_clean(summary: &ProjectRenderSummary) {
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
}

/// Write a fixture into a fresh temp dir and render it fully. The temp
/// dir is leaked so the test can inspect the output.
fn render_project(fixture: impl FnOnce(&Path)) -> PathBuf {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);
    let summary = render_in(&project_dir, RenderMode::Full).expect("pipeline should succeed");
    assert_clean(&summary);
    std::mem::forget(temp);
    project_dir
}

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

const BUNDLE_DIR: &str = "_site/site_libs/quarto-search/pagefind";
const MANIFEST: &str = "_site/site_libs/quarto-search/manifest.json";
const SEARCH_JSON: &str = "_site/search.json";

/// The shared fixture: navbar + sidebar, three indexable pages (one
/// with nested sections, two in a subdirectory section), a draft, a
/// `search: false` page, and a 404 page.
fn search_site(dir: &Path) {
    write(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n\
         website:\n  title: \"Test Site\"\n\
         \x20 navbar:\n    left:\n      - index.qmd\n      - docs/guide.qmd\n\
         \x20 sidebar:\n    contents:\n\
         \x20     - index.qmd\n\
         \x20     - section: \"Guide\"\n        contents:\n          - docs/guide.qmd\n          - docs/intro.qmd\n",
    );
    write(
        &dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\n\
         Intro paragraph before any section.\n\n\
         ## First section\n\n\
         Text in first section.\n\n\
         ### Nested heading\n\n\
         Nested text under the third-level heading.\n\n\
         ## Second section {#custom-id}\n\n\
         Second section text.\n",
    );
    write(
        &dir.join("docs/guide.qmd"),
        "---\ntitle: Guide page\n---\n\nGuide page body about rendering.\n",
    );
    write(
        &dir.join("docs/intro.qmd"),
        "---\ntitle: Intro\n---\n\nIntroductory words.\n",
    );
    write(
        &dir.join("secret.qmd"),
        "---\ntitle: Secret\ndraft: true\n---\n\nUnpublished draft text.\n",
    );
    write(
        &dir.join("hidden.qmd"),
        "---\ntitle: Hidden\nsearch: false\n---\n\nOpted out of search.\n",
    );
    write(
        &dir.join("404.qmd"),
        "---\ntitle: Not Found\n---\n\nNo such page here.\n",
    );
}

const SEARCH_JSON_ON: &str = "website:\n  title: \"Test Site\"\n  search:\n    search-json: true\n";
const SITE_TITLE: &str = "website:\n  title: \"Test Site\"\n";

/// Rewrite the fixture's `website:` header (`SITE_TITLE`) to `header`.
fn set_site_header(dir: &Path, header: &str) {
    let path = dir.join("_quarto.yml");
    let yml = read(&path);
    let current = if yml.contains(SEARCH_JSON_ON) {
        SEARCH_JSON_ON
    } else {
        SITE_TITLE
    };
    write(&path, &yml.replacen(current, header, 1));
}

/// [`search_site`] with `search-json: true`, for tests that read the
/// records.
fn records_site(dir: &Path) {
    search_site(dir);
    set_site_header(dir, SEARCH_JSON_ON);
}

fn records(project_dir: &Path) -> Vec<serde_json::Value> {
    read_json(&project_dir.join(SEARCH_JSON))
        .as_array()
        .expect("search.json is an array")
        .clone()
}

fn record<'a>(recs: &'a [serde_json::Value], href: &str) -> &'a serde_json::Value {
    recs.iter()
        .find(|r| r["href"].as_str() == Some(href))
        .unwrap_or_else(|| {
            let hrefs: Vec<_> = recs.iter().map(|r| r["href"].to_string()).collect();
            panic!("no record for {href}; have {hrefs:?}")
        })
}

fn bundle_files(project_dir: &Path) -> HashSet<String> {
    let root = project_dir.join(BUNDLE_DIR);
    let mut out = HashSet::new();
    fn walk(dir: &Path, root: &Path, out: &mut HashSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                out.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    assert!(root.is_dir(), "bundle dir missing: {}", root.display());
    walk(&root, &root, &mut out);
    out
}

fn options_json(html: &str) -> serde_json::Value {
    let marker = "<script id=\"quarto-search-options\" type=\"application/json\">";
    let start = html
        .find(marker)
        .unwrap_or_else(|| panic!("no search options script in:\n{html}"))
        + marker.len();
    let end = html[start..].find("</script>").unwrap() + start;
    serde_json::from_str(&html[start..end]).expect("search options JSON parses")
}

// ═══════════════════════════════════════════════════════════════════
// Bundle + manifest
// ═══════════════════════════════════════════════════════════════════

#[test]
fn search_bundle_emitted_by_default_for_website() {
    let project_dir = render_project(search_site);

    let files = bundle_files(&project_dir);
    for required in ["pagefind-entry.json", "pagefind.js", "wasm.en.pagefind"] {
        assert!(
            files.contains(required),
            "bundle lacks {required}; have {files:?}"
        );
    }
    assert!(
        files.iter().any(|f| f.ends_with(".pf_meta")),
        "no meta file: {files:?}"
    );
    assert!(
        files
            .iter()
            .any(|f| f.starts_with("index/") && f.ends_with(".pf_index")),
        "no index chunk: {files:?}"
    );
    assert!(
        files
            .iter()
            .any(|f| f.starts_with("fragment/") && f.ends_with(".pf_fragment")),
        "no fragments: {files:?}"
    );

    let entry = read_json(&project_dir.join(BUNDLE_DIR).join("pagefind-entry.json"));
    assert_eq!(entry["version"].as_str(), Some("1.5.2"));

    let manifest = read_json(&project_dir.join(MANIFEST));
    assert_eq!(manifest["version"].as_u64(), Some(1));
    assert_eq!(manifest["pagefind"].as_str(), Some("1.5.2"));
    assert_eq!(manifest["quarto"].as_str(), Some(quarto_core::version()));
    assert_eq!(manifest["title"].as_str(), Some("Test Site"));
    assert_eq!(manifest["bundle"].as_str(), Some("pagefind/"));
    assert_eq!(manifest["languages"][0].as_str(), Some("en"));
    assert_eq!(manifest["pages"].as_u64(), Some(3), "index, guide, intro");
}

#[test]
fn website_search_false_disables_everything() {
    let project_dir = render_project(|dir| {
        search_site(dir);
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Test Site\"\n  search: false\n\
             \x20 navbar:\n    left:\n      - index.qmd\n",
        );
    });
    assert!(!project_dir.join("_site/site_libs/quarto-search").exists());
    assert!(!project_dir.join(SEARCH_JSON).exists());
    let html = read(&project_dir.join("_site/index.html"));
    assert_not_contains(&html, "quarto-search-options", "search: false");
    assert_not_contains(&html, "id=\"quarto-search\"", "search: false");
}

// ═══════════════════════════════════════════════════════════════════
// search.json (Quarto 1 shape)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn search_json_records_follow_quarto1_shape() {
    let project_dir = render_project(records_site);
    let recs = records(&project_dir);

    // Page record: text before the first level-2 section.
    let page = record(&recs, "index.html");
    assert_eq!(page["objectID"].as_str(), Some("index.html"));
    assert_eq!(page["title"].as_str(), Some("Home"));
    assert_eq!(page["section"].as_str(), Some(""));
    assert_contains(
        page["text"].as_str().unwrap(),
        "Intro paragraph before any section.",
        "page text",
    );
    assert_not_contains(page["text"].as_str().unwrap(), "First section", "page text");

    // Level-2 section records, h3 folded in.
    let first = record(&recs, "index.html#first-section");
    assert_eq!(first["objectID"].as_str(), Some("index.html#first-section"));
    assert_eq!(first["title"].as_str(), Some("Home"));
    assert_eq!(first["section"].as_str(), Some("First section"));
    let text = first["text"].as_str().unwrap();
    assert_contains(text, "Text in first section.", "section text");
    assert_contains(text, "Nested heading", "h3 folded into its level-2 section");
    assert_contains(
        text,
        "Nested text under the third-level heading.",
        "h3 body folded",
    );
    assert_not_contains(
        text,
        "First section",
        "section text excludes its own heading",
    );

    let second = record(&recs, "index.html#custom-id");
    assert_eq!(second["section"].as_str(), Some("Second section"));
    assert_contains(
        second["text"].as_str().unwrap(),
        "Second section text.",
        "custom id",
    );

    // A page with no sections is a single record.
    let guide = record(&recs, "docs/guide.html");
    assert_eq!(guide["title"].as_str(), Some("Guide page"));
    assert_eq!(guide["section"].as_str(), Some(""));
    assert_contains(
        guide["text"].as_str().unwrap(),
        "Guide page body about rendering.",
        "guide text",
    );

    // Crumbs: sidebar trail (outermost first, page last) with the
    // first navbar item that resolves to the same sidebar merged in
    // front, Quarto 1's `merge-navbar-crumbs` default.
    let crumbs: Vec<&str> = guide["crumbs"]
        .as_array()
        .expect("guide record has crumbs")
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert_eq!(crumbs, vec!["Home", "Guide", "Guide page"]);
}

#[test]
fn merge_navbar_crumbs_false_keeps_sidebar_trail_only() {
    let project_dir = render_project(|dir| {
        search_site(dir);
        let yml = read(&dir.join("_quarto.yml"));
        write(
            &dir.join("_quarto.yml"),
            &yml.replace(
                "website:\n  title: \"Test Site\"\n",
                "website:\n  title: \"Test Site\"\n  search:\n    merge-navbar-crumbs: false\n\
                 \x20   search-json: true\n",
            ),
        );
    });
    let recs = records(&project_dir);
    let guide = record(&recs, "docs/guide.html");
    let crumbs: Vec<&str> = guide["crumbs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert_eq!(crumbs, vec!["Guide", "Guide page"]);
}

#[test]
fn search_excludes_drafts_search_false_and_404() {
    let project_dir = render_project(records_site);
    let recs = records(&project_dir);
    let hrefs: Vec<&str> = recs.iter().map(|r| r["href"].as_str().unwrap()).collect();
    for excluded in ["secret.html", "hidden.html", "404.html"] {
        assert!(
            !hrefs.iter().any(|h| h.starts_with(excluded)),
            "{excluded} must not be indexed; hrefs: {hrefs:?}"
        );
    }
    // The excluded text must not leak into the bundle either. Every
    // fragment is gzip-compressed CBOR with the page text as plain
    // bytes inside.
    let bundle = project_dir.join(BUNDLE_DIR);
    let mut all_text = String::new();
    for entry in std::fs::read_dir(bundle.join("fragment")).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        let mut decoder = flate2::read::GzDecoder::new(bytes.as_slice());
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut out).unwrap();
        all_text.push_str(&String::from_utf8_lossy(&out));
    }
    assert_contains(&all_text, "Guide page body", "indexed text in fragments");
    for leak in [
        "Unpublished draft text",
        "Opted out of search",
        "No such page here",
    ] {
        assert_not_contains(&all_text, leak, "excluded page text in fragments");
    }
    assert_eq!(
        read_json(&project_dir.join(MANIFEST))["pages"].as_u64(),
        Some(3)
    );
}

// ═══════════════════════════════════════════════════════════════════
// Incremental renders
// ═══════════════════════════════════════════════════════════════════

#[test]
fn subset_render_keeps_skipped_pages_in_index() {
    let project_dir = render_project(records_site);
    let before = records(&project_dir);
    assert!(record(&before, "index.html#first-section")["text"].is_string());

    // Re-render only the guide page; the index must still cover the
    // pages that were not rendered this run.
    let summary = render_in(
        &project_dir,
        RenderMode::Subset(HashSet::from([project_dir.join("docs/guide.qmd")])),
    )
    .expect("subset render succeeds");
    assert_clean(&summary);

    let after = records(&project_dir);
    assert_eq!(
        record(&after, "index.html#first-section")["text"],
        record(&before, "index.html#first-section")["text"],
        "skipped page kept from the previous run"
    );
    assert_eq!(
        record(&after, "docs/guide.html")["text"],
        record(&before, "docs/guide.html")["text"]
    );
    assert_eq!(after.len(), before.len());
    assert_eq!(
        read_json(&project_dir.join(MANIFEST))["pages"].as_u64(),
        Some(3)
    );
    assert!(
        project_dir
            .join(BUNDLE_DIR)
            .join("pagefind-entry.json")
            .exists()
    );
}

#[test]
fn subset_render_drops_sections_the_page_lost() {
    let project_dir = render_project(records_site);
    assert!(
        records(&project_dir)
            .iter()
            .any(|r| r["href"].as_str() == Some("index.html#custom-id"))
    );

    // The page loses its second section; a Subset render of just that
    // page must not leave the stale anchor behind (Quarto 1 did).
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nIntro paragraph before any section.\n\n\
         ## First section\n\nText in first section.\n",
    );
    let summary = render_in(
        &project_dir,
        RenderMode::Subset(HashSet::from([project_dir.join("index.qmd")])),
    )
    .expect("subset render succeeds");
    assert_clean(&summary);
    let recs = records(&project_dir);
    assert!(
        !recs
            .iter()
            .any(|r| r["href"].as_str() == Some("index.html#custom-id")),
        "stale section anchor pruned"
    );
    assert!(
        recs.iter()
            .any(|r| r["href"].as_str() == Some("docs/guide.html"))
    );
}

// ═══════════════════════════════════════════════════════════════════
// Per-page wiring
// ═══════════════════════════════════════════════════════════════════

#[test]
fn page_head_carries_search_options_and_offset() {
    let project_dir = render_project(search_site);

    let index = read(&project_dir.join("_site/index.html"));
    assert_contains(
        &index,
        "<meta name=\"quarto:offset\" content=\"./\">",
        "root page offset",
    );
    let opts = options_json(&index);
    assert_eq!(opts["location"].as_str(), Some("navbar"));
    assert_eq!(opts["type"].as_str(), Some("overlay"));
    assert_eq!(opts["panel-placement"].as_str(), Some("end"));
    assert_eq!(opts["limit"].as_u64(), Some(50));
    assert_eq!(opts["collapse-after"].as_u64(), Some(3));
    assert_eq!(opts["copy-button"].as_bool(), Some(false));
    assert_eq!(opts["show-item-context"].as_bool(), Some(false));
    assert_eq!(
        opts["keyboard-shortcut"],
        serde_json::json!(["f", "/", "s"]),
        "default shortcut list"
    );
    assert_eq!(opts["language"]["search-label"].as_str(), Some("Search"));
    assert_eq!(
        opts["language"]["search-no-results-text"].as_str(),
        Some("No results")
    );
    assert_eq!(
        opts["bundle"].as_str(),
        Some("site_libs/quarto-search/pagefind/"),
        "site-root-relative bundle path"
    );
    assert_eq!(opts["federate"], serde_json::json!([]));
    assert!(opts.get("algolia").is_none() || opts["algolia"].is_null());

    // The scripts ship from the lib dir; the engine runtime is loaded
    // lazily by quarto-search.js from the bundle, not as a page script.
    assert_contains(
        &index,
        "site_libs/quarto-search/quarto-search.js",
        "ui script",
    );
    assert_contains(
        &index,
        "site_libs/quarto-search/autocomplete.umd.js",
        "widget script",
    );

    // A page one directory down points back at the root.
    let guide = read(&project_dir.join("_site/docs/guide.html"));
    assert_contains(
        &guide,
        "<meta name=\"quarto:offset\" content=\"../\">",
        "nested page offset",
    );
    assert_contains(
        &guide,
        "../site_libs/quarto-search/quarto-search.js",
        "nested script path",
    );
}

#[test]
fn search_options_reflect_user_config() {
    let project_dir = render_project(|dir| {
        search_site(dir);
        let yml = read(&dir.join("_quarto.yml"));
        write(
            &dir.join("_quarto.yml"),
            &yml.replace(
                "website:\n  title: \"Test Site\"\n",
                "website:\n  title: \"Test Site\"\n  search:\n    location: navbar\n    type: textbox\n\
                 \x20   limit: 20\n    collapse-after: false\n    copy-button: true\n\
                 \x20   keyboard-shortcut: \"k\"\n    show-item-context: parent\n",
            ),
        );
    });
    let opts = options_json(&read(&project_dir.join("_site/index.html")));
    assert_eq!(opts["type"].as_str(), Some("textbox"));
    assert_eq!(opts["limit"].as_u64(), Some(20));
    assert_eq!(opts["collapse-after"].as_bool(), Some(false));
    assert_eq!(opts["copy-button"].as_bool(), Some(true));
    assert_eq!(
        opts["keyboard-shortcut"],
        serde_json::json!(["k"]),
        "scalar wrapped"
    );
    assert_eq!(opts["show-item-context"].as_str(), Some("parent"));
}

#[test]
fn navbar_gets_overlay_search_box_by_default() {
    let project_dir = render_project(search_site);
    let html = read(&project_dir.join("_site/index.html"));
    let navbar_start = html.find("<nav class=\"navbar").expect("navbar present");
    let navbar_end = html[navbar_start..].find("</nav>").unwrap() + navbar_start;
    let navbar = &html[navbar_start..navbar_end];
    assert_contains(navbar, "id=\"quarto-search\"", "search box in navbar");
    assert_contains(navbar, "title=\"Search\"", "localized label");
    // The results panel container always ships (textbox mode renders
    // into it; overlay mode ignores it).
    assert_contains(
        &html,
        "<div id=\"quarto-search-results\"></div>",
        "panel container",
    );
}

#[test]
fn navbar_search_false_removes_box_but_keeps_index() {
    let project_dir = render_project(|dir| {
        search_site(dir);
        let yml = read(&dir.join("_quarto.yml"));
        write(
            &dir.join("_quarto.yml"),
            &yml.replace("  navbar:\n", "  navbar:\n    search: false\n"),
        );
    });
    let html = read(&project_dir.join("_site/index.html"));
    assert_not_contains(&html, "id=\"quarto-search\"", "navbar.search: false");
    assert!(project_dir.join(MANIFEST).exists(), "index still written");
}

#[test]
fn sidebar_only_site_gets_textbox_search_in_sidebar() {
    let project_dir = render_project(|dir| {
        search_site(dir);
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"Test Site\"\n\
             \x20 sidebar:\n    contents:\n      - index.qmd\n      - docs/guide.qmd\n",
        );
    });
    let html = read(&project_dir.join("_site/index.html"));
    let opts = options_json(&html);
    assert_eq!(opts["location"].as_str(), Some("sidebar"));
    assert_eq!(opts["type"].as_str(), Some("textbox"));
    assert_eq!(opts["panel-placement"].as_str(), Some("start"));
    let sidebar_start = html.find("id=\"quarto-sidebar\"").expect("sidebar present");
    let sidebar = &html[sidebar_start..];
    assert_contains(sidebar, "id=\"quarto-search\"", "search box in sidebar");
    assert_contains(sidebar, "class=\"sidebar-search\"", "textbox wrapper");
}

#[test]
fn federate_config_reaches_options_json() {
    let project_dir = render_project(|dir| {
        search_site(dir);
        let yml = read(&dir.join("_quarto.yml"));
        write(
            &dir.join("_quarto.yml"),
            &yml.replace(
                "website:\n  title: \"Test Site\"\n",
                "website:\n  title: \"Test Site\"\n  search:\n    federate:\n\
                 \x20     - url: https://docs.example.org/child/\n        title: Child Docs\n        weight: 0.5\n\
                 \x20     - url: https://other.example.org/\n        bundle: pagefind/\n",
            ),
        );
    });
    let opts = options_json(&read(&project_dir.join("_site/index.html")));
    assert_eq!(
        opts["federate"],
        serde_json::json!([
            {"url": "https://docs.example.org/child/", "title": "Child Docs", "weight": 0.5},
            {"url": "https://other.example.org/", "bundle": "pagefind/"}
        ])
    );
}

// ═══════════════════════════════════════════════════════════════════
// Books
// ═══════════════════════════════════════════════════════════════════

/// A two-chapter book; `book_extra` is appended under `book:`.
fn minibook(dir: &Path, book_extra: &str) {
    write(
        &dir.join("_quarto.yml"),
        &format!(
            "project:\n  type: book\n  output-dir: _site\n\
             book:\n  title: \"Minibook\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n\
             {book_extra}"
        ),
    );
    write(
        &dir.join("index.qmd"),
        "# Welcome {.unnumbered}\n\nWelcome to the book about marmots.\n",
    );
    write(
        &dir.join("ch1.qmd"),
        "# Chapter One\n\nMarmots whistle.\n\n## Habitat\n\nAlpine meadows.\n",
    );
}

/// `book.search` reaches `website.search` wholesale, so a book's
/// `search.json` follows the same default as a website's.
#[test]
fn book_project_writes_no_search_json_by_default() {
    let project_dir = render_project(|dir| minibook(dir, ""));
    assert!(project_dir.join(MANIFEST).exists(), "bundle still built");
    assert!(!project_dir.join(SEARCH_JSON).exists());
}

/// Books share the website machinery (`book.search` is copied into
/// `website.search` by the book pre-render), so a book gets the
/// bundle, the manifest, and chapter records too.
#[test]
fn book_project_gets_a_search_index() {
    let project_dir = render_project(|dir| minibook(dir, "  search:\n    search-json: true\n"));
    assert!(
        project_dir
            .join(BUNDLE_DIR)
            .join("pagefind-entry.json")
            .exists()
    );
    let manifest = read_json(&project_dir.join(MANIFEST));
    assert_eq!(manifest["title"].as_str(), Some("Minibook"));
    assert_eq!(manifest["pages"].as_u64(), Some(2));
    let recs = records(&project_dir);
    let hrefs: Vec<&str> = recs.iter().map(|r| r["href"].as_str().unwrap()).collect();
    assert!(hrefs.contains(&"index.html"), "{hrefs:?}");
    assert!(hrefs.contains(&"ch1.html"), "{hrefs:?}");
    assert!(hrefs.contains(&"ch1.html#habitat"), "{hrefs:?}");
    let html = read(&project_dir.join("_site/ch1.html"));
    assert_contains(&html, "quarto-search-options", "book page wiring");
}

// ═══════════════════════════════════════════════════════════════════
// Algolia backend
// ═══════════════════════════════════════════════════════════════════

/// A complete `algolia` block switches the page's backend: the options
/// JSON carries the Algolia settings (plus `libDir`), the head loads
/// the Algolia client from the CDN, the Autocomplete preset ships from
/// the lib dir, and the logo is copied when `show-logo` is on.
/// `search.json` is written by default (it is what users upload to
/// Algolia).
#[test]
fn algolia_config_wires_client_preset_and_logo() {
    let project_dir = render_project(|dir| {
        search_site(dir);
        let yml = read(&dir.join("_quarto.yml"));
        write(
            &dir.join("_quarto.yml"),
            &yml.replace(
                "website:\n  title: \"Test Site\"\n",
                "website:\n  title: \"Test Site\"\n  search:\n    algolia:\n\
                 \x20     index-name: idx\n      application-id: app\n      search-only-api-key: key\n\
                 \x20     show-logo: true\n      analytics-events: true\n      params:\n        hitsPerPage: 5\n",
            ),
        );
    });
    let html = read(&project_dir.join("_site/index.html"));
    let opts = options_json(&html);
    assert_eq!(opts["algolia"]["index-name"].as_str(), Some("idx"));
    assert_eq!(opts["algolia"]["application-id"].as_str(), Some("app"));
    assert_eq!(opts["algolia"]["search-only-api-key"].as_str(), Some("key"));
    assert_eq!(opts["algolia"]["show-logo"].as_bool(), Some(true));
    assert_eq!(opts["algolia"]["libDir"].as_str(), Some("site_libs"));
    assert_eq!(opts["algolia"]["params"]["hitsPerPage"].as_u64(), Some(5));
    assert_contains(
        &html,
        "https://cdn.jsdelivr.net/npm/algoliasearch@4.5.1/dist/algoliasearch-lite.umd.js",
        "algolia client",
    );
    assert_contains(&html, "search-insights.iife.min.js", "insights loader");
    assert_contains(
        &html,
        "site_libs/quarto-search/autocomplete-preset-algolia.umd.js",
        "preset script",
    );
    assert!(
        project_dir
            .join("_site/site_libs/quarto-search/search-by-algolia.svg")
            .exists()
    );
    assert!(
        project_dir.join(SEARCH_JSON).exists(),
        "search.json still written"
    );
}

/// An incomplete `algolia` block is reported and ignored; the page
/// keeps the built-in backend.
#[test]
fn algolia_misconfigured_falls_back_to_builtin_search() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    search_site(&project_dir);
    let yml = read(&project_dir.join("_quarto.yml"));
    write(
        &project_dir.join("_quarto.yml"),
        &yml.replace(
            "website:\n  title: \"Test Site\"\n",
            "website:\n  title: \"Test Site\"\n  search:\n    algolia:\n      index-name: idx\n",
        ),
    );
    let summary = render_in(&project_dir, RenderMode::Full).expect("render succeeds");
    assert_clean(&summary);
    let warned = summary
        .project_diagnostics
        .iter()
        .any(|d| d.code.as_deref() == Some("Q-5-40"));
    assert!(
        warned,
        "Q-5-40 reported; diagnostics: {:?}",
        summary.project_diagnostics
    );
    let html = read(&project_dir.join("_site/index.html"));
    let opts = options_json(&html);
    assert!(
        opts.get("algolia").is_none(),
        "algolia dropped from options"
    );
    assert_not_contains(&html, "algoliasearch-lite", "no CDN client");
    assert!(
        !project_dir
            .join("_site/site_libs/quarto-search/autocomplete-preset-algolia.umd.js")
            .exists()
    );
    std::mem::forget(temp);
}

// ═══════════════════════════════════════════════════════════════════
// When search.json is written (bd-cdehspam)
// ═══════════════════════════════════════════════════════════════════

const ALGOLIA_HEADER: &str = "website:\n  title: \"Test Site\"\n  search:\n    algolia:\n\
     \x20     index-name: idx\n      application-id: app\n      search-only-api-key: key\n";

#[test]
fn search_json_absent_by_default_without_algolia() {
    let project_dir = render_project(search_site);
    assert!(project_dir.join(MANIFEST).exists(), "bundle still built");
    assert!(!project_dir.join(SEARCH_JSON).exists());
}

#[test]
fn search_json_true_writes_without_algolia() {
    let project_dir = render_project(records_site);
    // index (page + 2 sections), guide, intro.
    let recs = records(&project_dir);
    assert_eq!(recs.len(), 5, "{recs:?}");
}

#[test]
fn search_json_false_suppresses_even_with_algolia() {
    let project_dir = render_project(|dir| {
        search_site(dir);
        set_site_header(dir, &format!("{ALGOLIA_HEADER}    search-json: false\n"));
    });
    let opts = options_json(&read(&project_dir.join("_site/index.html")));
    assert_eq!(opts["algolia"]["index-name"].as_str(), Some("idx"));
    assert!(!project_dir.join(SEARCH_JSON).exists());
}

/// Turning `search.json` off removes the file a previous run wrote,
/// but keeps the bundle and the per-page capture cache.
#[test]
fn search_json_removed_when_setting_flips_off() {
    let project_dir = render_project(records_site);
    assert!(project_dir.join(SEARCH_JSON).exists());

    set_site_header(&project_dir, SITE_TITLE);
    assert_clean(&render_in(&project_dir, RenderMode::Full).unwrap());
    assert!(!project_dir.join(SEARCH_JSON).exists());
    assert!(project_dir.join(MANIFEST).exists());
    let cache = project_dir.join(".quarto/search/pages");
    assert!(
        std::fs::read_dir(&cache).is_ok_and(|mut d| d.next().is_some()),
        "capture cache kept"
    );

    // And back on: written again.
    set_site_header(&project_dir, SEARCH_JSON_ON);
    assert_clean(&render_in(&project_dir, RenderMode::Full).unwrap());
    assert!(project_dir.join(SEARCH_JSON).exists());
}

/// A `search.json` q2 wrote but someone changed since is not removed.
#[test]
fn edited_search_json_survives_flip_off() {
    let project_dir = render_project(records_site);
    write(&project_dir.join(SEARCH_JSON), "[\"hand-edited\"]");
    set_site_header(&project_dir, SITE_TITLE);
    assert_clean(&render_in(&project_dir, RenderMode::Full).unwrap());
    assert_eq!(read(&project_dir.join(SEARCH_JSON)), "[\"hand-edited\"]");
}

/// A `search.json` q2 never wrote is not removed.
#[test]
fn foreign_search_json_survives() {
    let project_dir = render_project(search_site);
    write(&project_dir.join(SEARCH_JSON), "[]");
    assert_clean(&render_in(&project_dir, RenderMode::Full).unwrap());
    assert_eq!(read(&project_dir.join(SEARCH_JSON)), "[]");
}

/// A site last rendered by the v1 writer (boolean state, no hash) has
/// its `search.json` removed once when the file is no longer wanted.
#[test]
fn v1_state_search_json_is_removed() {
    let project_dir = render_project(search_site);
    write(&project_dir.join(SEARCH_JSON), "[]");
    write(
        &project_dir.join(".quarto/search/state.json"),
        "{\"version\":1,\"search_json\":true}",
    );
    assert_clean(&render_in(&project_dir, RenderMode::Full).unwrap());
    assert!(!project_dir.join(SEARCH_JSON).exists());
}

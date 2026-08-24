/*
 * tests/integration/listing_inline_records.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for inline `contents:` records
 * (bd-listing-inline-contents-tyy446ze). Mirrors the fixtures in
 * `claude-notes/plans/listing-inline-contents-investigation/`.
 */

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Drive a fresh project through the full Pass-1/Pass-2 pipeline.
/// Returns the project dir and the per-file results (kept whole so
/// tests can inspect both the rendered HTML and the diagnostics).
fn render_project(fixture: impl FnOnce(&std::path::Path)) -> (PathBuf, Vec<RenderToFileResult>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    assert!(
        !project.is_single_file,
        "test expected a multi-file project"
    );

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
    let summary = pollster::block_on(pipeline.run()).expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures,
    );

    std::mem::forget(temp);
    (project_dir, summary.outputs)
}

/// Rendered HTML for the output whose path ends with
/// `relative_output` (forward-slash, e.g. `"sub/index.html"`).
fn html_for(outputs: &[RenderToFileResult], relative_output: &str) -> String {
    let suffix: PathBuf = relative_output.split('/').collect();
    let out = outputs
        .iter()
        .find(|o| o.output_path.ends_with(&suffix))
        .unwrap_or_else(|| {
            panic!(
                "no output ending in `{}`; have: {:?}",
                relative_output,
                outputs.iter().map(|o| &o.output_path).collect::<Vec<_>>()
            )
        });
    read(&out.output_path)
}

/// Titles of the listing items in a rendered page, in DOM order.
fn listing_titles(html: &str) -> Vec<String> {
    let mut titles = Vec::new();
    let marker = "listing-title\">";
    let mut rest = html;
    while let Some(i) = rest.find(marker) {
        rest = &rest[i + marker.len()..];
        if let Some(end) = rest.find('<') {
            titles.push(rest[..end].to_string());
        }
    }
    titles
}

/// Every diagnostic code attached to any per-page output.
fn all_diag_codes(outputs: &[RenderToFileResult]) -> Vec<String> {
    outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter())
        .filter_map(|d| d.code.clone())
        .collect()
}

fn assert_no_code(outputs: &[RenderToFileResult], code: &str) {
    let codes = all_diag_codes(outputs);
    assert!(
        !codes.iter().any(|c| c == code),
        "expected no {} diagnostics, got codes: {:?}",
        code,
        codes
    );
}

const PROJECT: &str = "project:\n  type: website\nwebsite:\n  title: Inline\n";

fn stub(title: &str) -> String {
    format!("---\ntitle: \"{title}\"\n---\nStub page.\n")
}

/// `repro/`: records with `path:` — titles come from the YAML, links
/// from the documents, no Q-12-2.
#[test]
fn records_with_path_render_from_yaml_and_link_to_documents() {
    let (_dir, outputs) = render_project(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(&p.join("download.qmd"), &stub("Download stub"));
        write(&p.join("features.qmd"), &stub("Features stub"));
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  id: cards\n  type: default\n  sort: false\n  contents:\n    - title: \"Get started\"\n      description: \"Download and install Positron\"\n      path: \"download.qmd\"\n    - title: \"Explore Features\"\n      description: \"Discover key Positron features\"\n      path: \"features.qmd\"\n---\n\nBody before the listing.\n",
        );
    });
    let host = html_for(&outputs, "index.html");
    assert_eq!(
        listing_titles(&host),
        vec!["Get started", "Explore Features"]
    );
    assert!(host.contains("href=\"download.html\""), "{host}");
    assert!(
        host.contains("Download and install Positron"),
        "record description wins: {host}"
    );
    assert_no_code(&outputs, "Q-12-2");
    assert_no_code(&outputs, "Q-12-19");
    assert_no_code(&outputs, "Q-12-20");
}

/// `mixed/` with `sort: false`: declared order, record first.
#[test]
fn mixed_record_and_glob_keep_declared_order() {
    let (_dir, outputs) = render_project(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(&p.join("download.qmd"), &stub("Download stub"));
        write(&p.join("features.qmd"), &stub("Features stub"));
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  id: cards\n  sort: false\n  contents:\n    - title: \"Get started\"\n      path: \"download.qmd\"\n    - \"features.qmd\"\n---\n",
        );
    });
    let host = html_for(&outputs, "index.html");
    assert_eq!(listing_titles(&host), vec!["Get started", "Features stub"]);
}

/// `linkonly/` (the Positron shape): no `path:`, custom keys, and a
/// custom doctemplate reading them flat.
///
/// Note `listing_titles` scans for the literal `listing-title">`, which
/// matches an unlinked heading only because `class` is the last attribute
/// the HTML writer emits (`pampa/src/writers/html.rs:505-532` orders
/// `id`, `class`, then key-value attrs). If a heading ever gains a kv
/// attribute this returns `[]` and the assertion below fails opaquely —
/// read the emitted HTML before assuming the feature broke.
#[test]
fn link_only_records_render_unlinked_cards_and_custom_template_reads_flat_keys() {
    let (_dir, outputs) = render_project(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(
            &p.join("card.template"),
            "$for(items)$\n```{=html}\n<a class=\"card\" href=\"$items.link$\"><i class=\"$items.icon$\"></i>$items.title$</a>\n```\n$endfor$\n",
        );
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  - id: plain\n    type: default\n    contents:\n      - title: \"Get started\"\n        description: \"Download and install Positron\"\n        icon: \"bi-rocket-takeoff\"\n        link: \"https://positron.posit.co/download.html\"\n  - id: custom\n    type: custom\n    template: card.template\n    contents:\n      - title: \"Migrate from RStudio\"\n        icon: \"bi-arrow-left-right\"\n        link: \"https://positron.posit.co/rstudio-rosetta-stone.html\"\n---\n\n::: {#plain}\n:::\n\n::: {#custom}\n:::\n",
        );
    });
    let host = html_for(&outputs, "index.html");
    assert_eq!(listing_titles(&host), vec!["Get started"]);
    assert!(
        !host.contains("no-external listing-title"),
        "an unlinked title must not render as the anchor form: {host}"
    );
    assert!(
        host.contains("href=\"https://positron.posit.co/rstudio-rosetta-stone.html\""),
        "custom template read `$items.link$`: {host}"
    );
    assert!(
        host.contains("class=\"bi-arrow-left-right\""),
        "custom template read `$items.icon$`: {host}"
    );
    assert_no_code(&outputs, "Q-12-2");
    assert_no_code(&outputs, "Q-12-21");
    assert_no_code(&outputs, "Q-12-22");
}

/// A record `path:` that names no document warns Q-12-20 with a
/// did-you-mean and keeps the card.
#[test]
fn record_path_typo_warns_q_12_20_with_did_you_mean() {
    let (_dir, outputs) = render_project(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(&p.join("guide/download.qmd"), &stub("Download"));
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  contents:\n    - title: \"Get started\"\n      path: \"download.qmd\"\n---\n",
        );
    });
    let codes = all_diag_codes(&outputs);
    assert_eq!(
        codes.iter().filter(|c| *c == "Q-12-20").count(),
        1,
        "{codes:?}"
    );
    // `LinkRewriteTransform` may additionally report the dead `.qmd`
    // link it is handed (a Q-13 code); that is expected, not a
    // double report of the same problem.
    let host = html_for(&outputs, "index.html");
    assert_eq!(listing_titles(&host), vec!["Get started"]);
    let message = outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter())
        .find(|d| d.code.as_deref() == Some("Q-12-20"))
        .map(|d| format!("{d:?}"))
        .unwrap();
    assert!(
        message.contains("guide/download.qmd"),
        "did-you-mean: {message}"
    );
}

/// A YAML-file entry warns Q-12-23, not Q-12-19.
#[test]
fn yaml_file_contents_entry_warns_q_12_23() {
    let (_dir, outputs) = render_project(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(&p.join("items.yml"), "- title: From YAML\n");
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  contents:\n    - items.yml\n---\n",
        );
    });
    let codes = all_diag_codes(&outputs);
    assert!(codes.iter().any(|c| c == "Q-12-23"), "{codes:?}");
    assert_no_code(&outputs, "Q-12-19");
}

// ─────────────────────────────────────────────────────────────────
// Additional coverage (team-lead round beyond the brief): a record's
// `path:` must resolve against the directory of the file that
// *declared* it, per `claude-notes/designs/path-resolution-model.md`
// — the same provenance-based contract §D4's `resolve_record_path`
// implements via `BaseDirContext::base_dir_for`
// (`crates/quarto-core/src/glob/provenance.rs`). Every existing unit
// test in `listing_generate.rs` builds no `SourceContext`, so
// `base_dir_for` never takes its provenance branch there — it always
// falls back to the caller-supplied directory. These two fixtures
// drive a real `ProjectPipeline` with a real `SourceContext` so the
// provenance branch is actually exercised.
// ─────────────────────────────────────────────────────────────────

/// A record declared in `_quarto.yml` resolves its `path:` against
/// the *project root*, not the host page's directory — even though
/// the project-level `listing:` is inherited by every page
/// (mirrors `projmeta_glob_resolves_against_project_root` in
/// `listing_glob_resolution.rs`, but for a record's `path:` instead
/// of a glob).
///
/// Discrimination: a decoy document of the SAME name
/// (`sub/download.qmd`) sits right next to the host page. If
/// `base_dir_for` regressed to the fallback (host) directory instead
/// of taking the provenance branch, this test would still pass a
/// weaker "resolves to *some* download page" assertion — it would
/// silently link to the wrong document. Asserting the exact
/// project-relative href (`../download.html`, reaching past `sub/`)
/// and the negative absence of the bare sibling `download.html` is
/// what makes the two cases distinguishable.
#[test]
fn record_path_in_project_yaml_resolves_against_project_root() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\nwebsite:\n  title: Inline\nlisting:\n  id: proj\n  type: default\n  sort: false\n  contents:\n    - title: \"Shared Record\"\n      path: \"download.qmd\"\n",
        );
        write(&p.join("download.qmd"), &stub("Root Download"));
        // Decoy: same leaf name, sibling of the host page. A
        // fallback-to-host-dir regression would resolve here instead.
        write(&p.join("sub/download.qmd"), &stub("Sub Decoy"));
        write(
            &p.join("sub/viewer.qmd"),
            "---\ntitle: Viewer\n---\n\nBody.\n",
        );
    });

    let host = html_for(&outputs, "sub/viewer.html");
    assert_eq!(listing_titles(&host), vec!["Shared Record"]);
    assert!(
        host.contains("href=\"../download.html\""),
        "the _quarto.yml-declared record must resolve against the project \
         root (../download.html from sub/), not the host's own directory: {host}"
    );
    assert!(
        !host.contains("href=\"download.html\""),
        "must NOT resolve to the sub/ decoy — that would mean the record \
         fell back to the host directory instead of the project root: {host}"
    );
}

/// Contrast case: the identical relative text `"download.qmd"`,
/// declared in a *page's own front matter* instead of `_quarto.yml`,
/// resolves against that page's own directory — the opposite base
/// directory from the previous test, proving `base_dir_for` is
/// actually keyed on provenance and not just always picking the
/// project root.
#[test]
fn record_path_in_front_matter_resolves_against_host_dir_not_project_root() {
    let (_dir, outputs) = render_project(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        // Same leaf name at the project root, so a wrong resolution
        // (falling through to the project root) still finds a
        // document and would otherwise pass silently.
        write(&p.join("download.qmd"), &stub("Root Download"));
        write(&p.join("sub2/download.qmd"), &stub("Sub2 Download"));
        write(
            &p.join("sub2/local.qmd"),
            "---\ntitle: Local\nlisting:\n  id: localrecord\n  type: default\n  sort: false\n  contents:\n    - title: \"Shared Record\"\n      path: \"download.qmd\"\n---\n",
        );
    });

    let host = html_for(&outputs, "sub2/local.html");
    assert_eq!(listing_titles(&host), vec!["Shared Record"]);
    assert!(
        host.contains("href=\"download.html\""),
        "a record declared in the page's own front matter must resolve \
         against that page's directory (bare sibling download.html): {host}"
    );
    assert!(
        !host.contains("href=\"../download.html\""),
        "must NOT resolve to the project root — that is the OTHER \
         test's contract, not this one's: {host}"
    );
}

// ─────────────────────────────────────────────────────────────────
// Additional coverage: item-grid's unlinked subtitle/thumbnail/
// description branches (only item-default's unlinked title is
// exercised by `link_only_records_render_unlinked_cards_and_...`
// above).
// ─────────────────────────────────────────────────────────────────

/// A `type: grid` listing of a link-only record (no `path:`): title,
/// subtitle, thumbnail and description must all render unlinked —
/// their card classes intact, no anchor wrapping any of them.
#[test]
fn grid_type_renders_unlinked_subtitle_thumbnail_and_description() {
    let (_dir, outputs) = render_project(|p| {
        write(&p.join("_quarto.yml"), PROJECT);
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  id: cards\n  type: grid\n  contents:\n    - title: \"Card Title\"\n      subtitle: \"Card Subtitle\"\n      description: \"Card description text\"\n      image: \"https://example.com/thumb.png\"\n---\n",
        );
    });
    let host = html_for(&outputs, "index.html");

    let card_start = host
        .find("quarto-grid-item")
        .unwrap_or_else(|| panic!("expected a grid card in the output: {host}"));
    let card = &host[card_start..];

    // No `path:` on the record → the item has no link target at
    // all, so nothing in its card may be wrapped by an anchor. This
    // is the discriminating assertion: the *linked* form of each of
    // these elements (exercised elsewhere, e.g.
    // `records_with_path_render_from_yaml_and_link_to_documents`)
    // wraps title/subtitle/description in `<a href="...">`; a broken
    // "unlinked" branch that fell through to the linked template arm
    // would trip this.
    assert!(
        !card.contains("<a "),
        "a link-only grid item (no `path:`) must render with no anchors \
         at all: {card}"
    );

    assert!(
        card.contains("no-anchor card-title listing-title"),
        "unlinked title keeps its card classes: {card}"
    );
    assert!(
        card.contains("card-subtitle listing-subtitle"),
        "unlinked subtitle keeps its card classes: {card}"
    );
    assert!(
        card.contains("thumbnail-image"),
        "unlinked thumbnail still renders the image: {card}"
    );
    assert!(
        card.contains("listing-description"),
        "unlinked description keeps its card class: {card}"
    );
    assert!(card.contains("Card Subtitle"), "{card}");
    assert!(card.contains("Card description text"), "{card}");
}

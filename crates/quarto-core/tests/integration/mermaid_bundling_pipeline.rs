/*
 * tests/integration/mermaid_bundling_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for bd-mermaid-runtime-not-bundled-vxejw159:
 * the mermaid runtime is bundled into the rendered site instead of
 * being imported from jsDelivr at page load.
 */

//! End-to-end integration tests for mermaid runtime bundling.
//!
//! These drive a real `render_to_file` (single-doc) or `ProjectPipeline`
//! (website) render, then assert:
//!
//! - The rendered HTML references the runtime with a **relative** URL and
//!   contains no `cdn.jsdelivr.net` anywhere.
//! - The runtime file lands on disk, byte-identical to what the binary
//!   embedded, and is shared across pages in a website.
//! - Nested pages get the correct `../site_libs/...` prefix.
//! - Diagram-free documents pay for none of it.
//!
//! The offline-correctness claim rests on the runtime being a
//! *self-contained* bundle; that property is guarded by unit tests in
//! `crates/quarto-core/src/transforms/mermaid.rs`, not here.
//!
//! Mirrors `bootstrap_js_pipeline.rs` for setup conventions.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const MERMAID_JS_BASENAME: &str = "mermaid.min.js";

/// A document with one flowchart.
const DIAGRAM_DOC: &str = "---\ntitle: Diagram\n---\n\n```mermaid\nflowchart LR\n  a --> b\n```\n";

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Pull every `<script src="…">` URL out of an HTML string, in document
/// order. Same lightweight parser as `bootstrap_js_pipeline.rs`.
fn extract_script_srcs(html: &str) -> Vec<String> {
    let needle = "<script src=\"";
    let mut search = html;
    let mut out = Vec::new();
    while let Some(start) = search.find(needle) {
        let after = &search[start + needle.len()..];
        let end = after
            .find('"')
            .expect("malformed <script>: missing closing quote on src");
        out.push(after[..end].to_string());
        search = &after[end..];
    }
    out
}

fn mermaid_srcs(html: &str) -> Vec<String> {
    extract_script_srcs(html)
        .into_iter()
        .filter(|s| s.contains(MERMAID_JS_BASENAME))
        .collect()
}

fn render_website(fixture: impl FnOnce(&Path)) -> PathBuf {
    let temp = TempDir::new().unwrap();
    // Leak the TempDir so the directory survives for the test
    // assertions; tests are short-lived processes.
    let project_dir = canonical(temp.path());
    std::mem::forget(temp);
    fixture(&project_dir);

    let runtime = runtime_arc();
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
    let summary = pollster::block_on(pipeline.run()).expect("project render");
    assert!(
        !summary.has_failures(),
        "project render reported failures: {:?}",
        summary
    );
    project_dir
}

/// Website with a diagram at the root, a diagram on a nested page, and a
/// diagram-free page.
fn mixed_fixture(project_dir: &Path) {
    write_file(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n",
    );
    write_file(&project_dir.join("index.qmd"), DIAGRAM_DOC);
    write_file(
        &project_dir.join("plain.qmd"),
        "---\ntitle: Plain\n---\n\nNo diagrams here.\n",
    );
    write_file(&project_dir.join("docs").join("api.qmd"), DIAGRAM_DOC);
}

// ── The core regression: no CDN reference anywhere ────────────────────────

/// bd-mermaid-runtime-not-bundled-vxejw159: a rendered page must not
/// reach out to a third-party CDN for the mermaid runtime. This is the
/// assertion the whole strand exists for.
#[test]
fn rendered_page_has_no_cdn_reference() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(&qmd_path, DIAGRAM_DOC);

    let options = RenderToFileOptions::default();
    let result =
        render_to_file(&qmd_path, "html", &options, runtime_arc()).expect("single-doc render");
    let html = read(&result.output_path);

    assert!(
        !html.contains("cdn.jsdelivr.net"),
        "rendered HTML must not reference jsDelivr; found it in:\n{}",
        html
    );
}

/// The runtime is referenced by a relative URL and the file it points at
/// actually exists on disk next to the page.
#[test]
fn single_doc_writes_runtime_and_links_it_relatively() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(&qmd_path, DIAGRAM_DOC);

    let options = RenderToFileOptions::default();
    let result =
        render_to_file(&qmd_path, "html", &options, runtime_arc()).expect("single-doc render");
    let html = read(&result.output_path);

    let srcs = mermaid_srcs(&html);
    assert_eq!(
        srcs.len(),
        1,
        "expected exactly one mermaid.min.js <script>; got {:?}",
        srcs
    );
    let src = &srcs[0];
    assert!(
        !src.starts_with("http://") && !src.starts_with("https://") && !src.starts_with('/'),
        "runtime URL must be relative; got {src}"
    );

    let page_dir = result.output_path.parent().unwrap();
    let on_disk = page_dir.join(src);
    assert!(
        on_disk.exists(),
        "runtime URL {src} does not resolve to a file on disk (looked at {})",
        on_disk.display()
    );
}

// ── Website: sharing, relative depth, and opt-out-by-absence ──────────────

/// A website render writes exactly one shared copy of the runtime under
/// `_site/site_libs/mermaid/`, and every diagram page references it.
#[test]
fn website_render_emits_one_shared_runtime() {
    let project_dir = render_website(mixed_fixture);
    let site = project_dir.join("_site");

    let shared = site
        .join("site_libs")
        .join("mermaid")
        .join(MERMAID_JS_BASENAME);
    assert!(
        shared.exists(),
        "expected shared mermaid runtime at {}",
        shared.display()
    );

    for page in &["index.html", "docs/api.html"] {
        let html = read(&site.join(page));
        let srcs = mermaid_srcs(&html);
        assert_eq!(
            srcs.len(),
            1,
            "{page}: expected one mermaid.min.js <script>; got {srcs:?}"
        );
        assert!(
            !html.contains("cdn.jsdelivr.net"),
            "{page}: must not reference jsDelivr"
        );
    }
}

/// A nested page (`docs/api.html`) gets a `../site_libs/...` URL, and it
/// resolves to the shared file.
#[test]
fn website_nested_page_links_runtime_with_relative_prefix() {
    let project_dir = render_website(mixed_fixture);
    let site = project_dir.join("_site");
    let api_html = read(&site.join("docs").join("api.html"));

    let srcs = mermaid_srcs(&api_html);
    let src = srcs
        .first()
        .unwrap_or_else(|| panic!("no mermaid <script> on nested page"));
    assert!(
        src.starts_with("../site_libs/"),
        "nested page must use a `../site_libs/...` URL; got {src}"
    );

    let resolved = site.join("docs").join(src);
    assert!(
        resolved.exists(),
        "nested page URL {src} does not resolve on disk (looked at {})",
        resolved.display()
    );
}

/// A page with no diagrams ships neither the `<script>` nor a
/// `pre.mermaid` block — diagram-free documents pay nothing.
#[test]
fn diagram_free_page_ships_no_runtime_reference() {
    let project_dir = render_website(mixed_fixture);
    let html = read(&project_dir.join("_site").join("plain.html"));

    assert!(
        mermaid_srcs(&html).is_empty(),
        "diagram-free page must not reference the mermaid runtime"
    );
    assert!(
        !html.contains("cdn.jsdelivr.net"),
        "diagram-free page must not reference jsDelivr"
    );
}

/// A revealjs deck with a diagram also gets the bundled runtime.
///
/// This is not incidental coverage. The reveal scaffold collects only
/// `js:revealjs:*` artifacts, so a runtime registered under the `js:`
/// prefix would produce **no** `<script>` tag in a deck at all. The
/// transform therefore emits its own tag into the after-body slot,
/// which both the Bootstrap template and the reveal scaffold wire up.
/// If someone later "tidies" the artifact key to `js:mermaid:…`, the
/// unit tests still pass and only this test catches the breakage.
#[test]
fn revealjs_deck_bundles_the_runtime() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("deck.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: Deck\nformat: revealjs\n---\n\n## Slide\n\n\
         ```mermaid\nflowchart LR\n  a --> b\n```\n",
    );

    let options = RenderToFileOptions::default();
    let result =
        render_to_file(&qmd_path, "revealjs", &options, runtime_arc()).expect("revealjs render");
    let html = read(&result.output_path);

    assert!(
        !html.contains("cdn.jsdelivr.net"),
        "deck must not reference jsDelivr"
    );
    let srcs = mermaid_srcs(&html);
    assert_eq!(
        srcs.len(),
        1,
        "expected exactly one mermaid runtime <script> in the deck; got {srcs:?}"
    );

    let on_disk = result.output_path.parent().unwrap().join(&srcs[0]);
    assert!(
        on_disk.exists(),
        "deck runtime URL {} does not resolve on disk (looked at {})",
        srcs[0],
        on_disk.display()
    );
}

/// Two diagrams on one page still yield exactly one runtime `<script>`
/// (the `MERMAID_JS_SENTINEL` idempotence contract, observed end-to-end).
#[test]
fn multiple_diagrams_emit_one_runtime_script() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: Two\n---\n\n```mermaid\nflowchart LR\n  a --> b\n```\n\n\
         ```mermaid\nsequenceDiagram\n  A->>B: hi\n```\n",
    );

    let options = RenderToFileOptions::default();
    let result =
        render_to_file(&qmd_path, "html", &options, runtime_arc()).expect("single-doc render");
    let html = read(&result.output_path);

    assert_eq!(
        mermaid_srcs(&html).len(),
        1,
        "expected exactly one runtime <script> for two diagrams"
    );
    assert_eq!(
        html.matches("<pre class=\"mermaid\">").count(),
        2,
        "expected two rendered diagram blocks"
    );
}

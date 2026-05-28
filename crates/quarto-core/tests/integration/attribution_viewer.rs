//! Phase A tests for `AttributionViewerTransform`.
//!
//! Pins the contract from
//! `claude-notes/plans/2026-05-14-attribution-auto-viewer.md`: when
//! attribution wrappers were produced (i.e. `AttributionRenderTransform`
//! populated `format_options.html.attribution_by_node`) and the YAML
//! opt-out was not set, the viewer transform appends an inline `<style>`
//! to `rendered.includes.header` and an inline `<script>` to
//! `rendered.includes.after-body`. The full HTML template wires those
//! slots into `<head>` / before-`</body>` respectively.
//!
//! All five tests RED at file creation: the stub transform returns
//! `unimplemented!()`. Phase 3 turns them green.

use std::collections::HashMap;
use std::sync::Arc;

use quarto_core::Format;
use quarto_core::attribution::{AttributionRecord, Identity, IdentityMap};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::transform::AstTransform;
use quarto_core::transforms::AttributionViewerTransform;
use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

fn make_project(dir: &std::path::Path) -> ProjectContext {
    ProjectContext {
        dir: dir.to_path_buf(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(dir.join("test.qmd"))],
        output_dir: dir.to_path_buf(),
    }
}

fn empty_pandoc() -> Pandoc {
    Pandoc {
        blocks: Vec::new(),
        meta: ConfigValue::new_map(Vec::new(), SourceInfo::default()),
    }
}

/// Build a non-empty `attribution_by_node` map. The keys / values
/// don't need to correspond to a real AST — the viewer transform
/// only checks `is_some()`, not the contents.
fn fixture_attribution_by_node() -> Arc<HashMap<usize, AttributionRecord>> {
    let mut m = HashMap::new();
    m.insert(
        42usize,
        AttributionRecord {
            actor: Arc::from("alice"),
            time: 1_700_000_000,
        },
    );
    Arc::new(m)
}

fn fixture_identities() -> Arc<IdentityMap> {
    let mut m = IdentityMap::new();
    m.insert(
        Arc::from("alice"),
        Identity {
            display_name: "Alice".to_string(),
            // Use a representative Tol Muted entry here; the exact
            // value need not match what `actor_color("alice")` would
            // emit — this fixture exists to exercise the viewer's
            // CSS-rule emission path with a known identity.
            color: "#CC6677".to_string(),
        },
    );
    Arc::new(m)
}

/// Collect the strings inside `meta.rendered.includes.<slot>`. Returns
/// an empty vec when the slot is absent (off-path invariant).
fn rendered_includes_slot(meta: &ConfigValue, slot: &str) -> Vec<String> {
    meta.get_path(&["rendered", "includes", slot])
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Build a RenderContext with attribution active (i.e. as if
/// `AttributionRenderTransform` had already run). The bool defaults
/// to `true` per `HtmlFormatOptions::default()`.
fn ctx_with_attribution_on<'a>(
    project: &'a ProjectContext,
    doc: &'a DocumentInfo,
    format: &'a Format,
    binaries: &'a BinaryDependencies,
) -> RenderContext<'a> {
    let mut ctx = RenderContext::new(project, doc, format, binaries);
    ctx.format_options.html.attribution_by_node = Some(fixture_attribution_by_node());
    ctx.format_options.html.attribution_identities = Some(fixture_identities());
    ctx
}

#[tokio::test]
async fn attribution_viewer_emits_includes_when_active() {
    let dir = std::env::temp_dir().join("attr-viewer-test-active");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = ctx_with_attribution_on(&project, &doc, &format, &binaries);

    let mut ast = empty_pandoc();
    AttributionViewerTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("viewer transform");

    let header = rendered_includes_slot(&ast.meta, "header");
    let after_body = rendered_includes_slot(&ast.meta, "after-body");

    assert_eq!(
        header.len(),
        1,
        "exactly one CSS include appended; got header = {:#?}",
        header
    );
    assert!(
        header[0].contains("<style"),
        "header include must be a <style> block; got: {}",
        header[0]
    );
    assert!(
        header[0].contains("q2-attr-badge"),
        "viewer CSS must mention .q2-attr-badge class; got: {}",
        header[0]
    );
    assert!(
        header[0].contains("<!-- quarto-attribution-viewer-css -->"),
        "viewer CSS must carry the dedup sentinel; got: {}",
        header[0]
    );
    // Per-actor rule emits `--attr-color` and `--attr-name` so the
    // browser paints colour via the cascade and viewer.js reads name
    // from computed style. One rule per distinct actor.
    assert!(
        header[0].contains("[data-attr-actor=\"alice\"]"),
        "header <style> must carry a rule for actor 'alice'; got: {}",
        header[0]
    );
    assert!(
        header[0].contains("--attr-color: #CC6677"),
        "alice's rule must carry --attr-color from the identity map; got: {}",
        header[0]
    );
    assert!(
        header[0].contains("--attr-name: \"Alice\""),
        "alice's rule must carry quoted --attr-name string; got: {}",
        header[0]
    );

    assert_eq!(
        after_body.len(),
        1,
        "exactly one JS include appended; got after-body = {:#?}",
        after_body
    );
    assert!(
        after_body[0].contains("<script"),
        "after-body include must be a <script> block; got: {}",
        after_body[0]
    );
    assert!(
        after_body[0].contains("data-attr-actor"),
        "viewer JS must mention data-attr-actor (the per-node attribute it selects on); got: {}",
        after_body[0]
    );
    assert!(
        after_body[0].contains("<!-- quarto-attribution-viewer-js -->"),
        "viewer JS must carry the dedup sentinel; got: {}",
        after_body[0]
    );
}

#[tokio::test]
async fn attribution_viewer_skips_when_attribution_off() {
    let dir = std::env::temp_dir().join("attr-viewer-test-off");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    // No attribution_by_node => attribution never ran. The default
    // `attribution_viewer_enabled = true` from `HtmlFormatOptions`
    // must NOT cause a spurious injection.
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    assert!(
        ctx.format_options.html.attribution_by_node.is_none(),
        "fixture invariant: attribution_by_node is None on the off path"
    );

    let mut ast = empty_pandoc();
    AttributionViewerTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("viewer transform");

    let header = rendered_includes_slot(&ast.meta, "header");
    let after_body = rendered_includes_slot(&ast.meta, "after-body");
    assert!(
        header.is_empty(),
        "off path must not append to header; got: {:#?}",
        header
    );
    assert!(
        after_body.is_empty(),
        "off path must not append to after-body; got: {:#?}",
        after_body
    );
    assert!(
        !matches!(&ast.meta.value, ConfigValueKind::Map(_))
            || ast.meta.get_path(&["rendered"]).is_none(),
        "off path must not even touch the `rendered.*` subtree"
    );
}

#[tokio::test]
async fn attribution_viewer_skips_when_viewer_disabled() {
    let dir = std::env::temp_dir().join("attr-viewer-test-disabled");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = ctx_with_attribution_on(&project, &doc, &format, &binaries);
    // YAML opt-out: `attribution: { source: git, viewer: false }`.
    ctx.format_options.html.attribution_viewer_enabled = false;

    let mut ast = empty_pandoc();
    AttributionViewerTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("viewer transform");

    let header = rendered_includes_slot(&ast.meta, "header");
    let after_body = rendered_includes_slot(&ast.meta, "after-body");
    assert!(
        header.is_empty(),
        "viewer: false must suppress the header include; got: {:#?}",
        header
    );
    assert!(
        after_body.is_empty(),
        "viewer: false must suppress the after-body include; got: {:#?}",
        after_body
    );
}

#[tokio::test]
async fn attribution_viewer_emits_when_no_matches() {
    // Attribution is on, the document has zero matched nodes (empty
    // by_node map). The transform still injects so the feature feels
    // alive on documents the author has just started editing.
    let dir = std::env::temp_dir().join("attr-viewer-test-empty-map");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    ctx.format_options.html.attribution_by_node =
        Some(Arc::new(HashMap::<usize, AttributionRecord>::new()));
    ctx.format_options.html.attribution_identities = Some(Arc::new(IdentityMap::new()));

    let mut ast = empty_pandoc();
    AttributionViewerTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("viewer transform");

    let header = rendered_includes_slot(&ast.meta, "header");
    let after_body = rendered_includes_slot(&ast.meta, "after-body");
    assert_eq!(
        header.len(),
        1,
        "empty by_node still injects CSS; got: {:#?}",
        header
    );
    assert_eq!(
        after_body.len(),
        1,
        "empty by_node still injects JS; got: {:#?}",
        after_body
    );
}

#[tokio::test]
async fn attribution_viewer_idempotent_on_rerun() {
    let dir = std::env::temp_dir().join("attr-viewer-test-idempotent");
    let project = make_project(&dir);
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = ctx_with_attribution_on(&project, &doc, &format, &binaries);

    let mut ast = empty_pandoc();
    let t = AttributionViewerTransform::new();
    t.transform(&mut ast, &mut ctx)
        .await
        .expect("first transform");
    t.transform(&mut ast, &mut ctx)
        .await
        .expect("second transform");

    let header = rendered_includes_slot(&ast.meta, "header");
    let after_body = rendered_includes_slot(&ast.meta, "after-body");
    assert_eq!(
        header.len(),
        1,
        "running twice must not double-inject CSS; got header = {:#?}",
        header
    );
    assert_eq!(
        after_body.len(),
        1,
        "running twice must not double-inject JS; got after-body = {:#?}",
        after_body
    );
}

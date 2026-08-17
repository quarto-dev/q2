/*
 * footer_generate.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Page-footer resolution transform.
//!
//! Reads the top-level `page-footer:` YAML from merged metadata
//! (Phase 3 Decision 1 — stays at the top level; it's feature-scoped
//! rather than website-scoped, so revealjs/single-doc users can
//! configure a footer without namespacing their config under
//! `website:`). Hands it to [`quarto_navigation::resolve_page_footer`]
//! and stores the result at `navigation.footer`.
//!
//! When a [`ProjectIndex`](crate::project::index::ProjectIndex) is
//! attached to the context, bare-href items inside the footer's
//! `left`/`center`/`right` regions (when those regions are
//! `FooterRegion::Items`) get their `text` enriched with the
//! referenced document's title — matching the sidebar and navbar
//! enrichment. Footer items do **not** get active marking (Phase 3
//! Decision 8 — matches Q1; footers are static cross-site chrome).
//!
//! ## Skip conditions
//!
//! - `page-footer: false` (affirmative disable).
//! - `page-footer` absent or `page-footer: true`.
//! - `navigation.footer` already populated (user override).

use quarto_navigation::{FooterRegion, NavigationItem, resolve_page_footer};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::is_feature_disabled;
use crate::transforms::navigation_enrich::enrich_navigation_items;
use crate::transforms::navigation_href::resolve_metadata_path;

pub struct FooterGenerateTransform;

impl FooterGenerateTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FooterGenerateTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for FooterGenerateTransform {
    fn name(&self) -> &str {
        "footer-generate"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "page-footer") {
            return Ok(());
        }

        if ast.meta.contains_path(&["navigation", "footer"]) {
            return Ok(());
        }

        let Some(mut footer) = resolve_page_footer(&ast.meta) else {
            return Ok(());
        };

        // bd-qor9a — resolve each footer item's href against the YAML
        // file it was authored in.
        if let Some(source_context) = ctx.source_context {
            resolve_region_hrefs(&mut footer.left, source_context, &ctx.project.dir);
            resolve_region_hrefs(&mut footer.center, source_context, &ctx.project.dir);
            resolve_region_hrefs(&mut footer.right, source_context, &ctx.project.dir);
        }

        if let Some(index) = ctx.project_index.as_deref() {
            enrich_footer_region(&mut footer.left, index);
            enrich_footer_region(&mut footer.center, index);
            enrich_footer_region(&mut footer.right, index);

            // Anything enrichment could not resolve to a project document
            // is display text, not a link (bd-page-footer-items-f4th80mj,
            // defect 2).
            //
            // Deliberately inside the index branch: demotion is a claim
            // that we *looked* for a matching document and did not find
            // one. With no index attached (single-file renders) nothing
            // resolves, so demoting there would turn every bare footer
            // item into text on no evidence at all.
            demote_unresolved_bare_items(&mut footer.left);
            demote_unresolved_bare_items(&mut footer.center);
            demote_unresolved_bare_items(&mut footer.right);
        }

        ast.meta
            .insert_path(&["navigation", "footer"], footer.to_config_value());

        Ok(())
    }
}

/// Turn every unresolved bare-scalar item in a region into display text
/// (bd-page-footer-items-f4th80mj, defect 2).
///
/// A bare scalar in a page-footer region is provisionally parsed as an
/// href so href resolution and index enrichment can run over it. If
/// enrichment found a matching document it filled `text`, and the item
/// stays a link. If `text` is still empty the scalar named no document —
/// it is a copyright line, an external URL, or a stale path — and Q1
/// renders it as plain `<li>` text. Previously q2 emitted an anchor whose
/// target was the sentence and whose body was empty.
///
/// Only items carrying [`NavigationItem::bare_text`] are eligible, so an
/// explicit `href:` that misses the index keeps its anchor. The caller
/// must only invoke this when a project index was actually consulted —
/// see the call site.
fn demote_unresolved_bare_items(region: &mut FooterRegion) {
    let FooterRegion::Items(items) = region else {
        return;
    };
    for item in items.iter_mut() {
        let Some(bare) = item.bare_text.take() else {
            continue;
        };
        if item.text.is_none() {
            item.text = Some(*bare);
            item.href = None;
        }
    }
}

/// Enrich items inside a footer region when the region carries a
/// `Vec<NavigationItem>`. `Text` and `Empty` regions are untouched —
/// body-content link rewriting is Phase 6's territory.
fn enrich_footer_region(region: &mut FooterRegion, index: &crate::project::index::ProjectIndex) {
    if let FooterRegion::Items(items) = region {
        enrich_navigation_items(items, index);
    }
}

/// bd-qor9a — resolve every item's href against the YAML file it
/// was authored in. Only touches `FooterRegion::Items`; `Text` and
/// `Empty` regions have no href to resolve.
fn resolve_region_hrefs(
    region: &mut FooterRegion,
    source_context: &quarto_source_map::SourceContext,
    project_root: &std::path::Path,
) {
    if let FooterRegion::Items(items) = region {
        resolve_item_hrefs(items, source_context, project_root);
    }
}

fn resolve_item_hrefs(
    items: &mut [NavigationItem],
    source_context: &quarto_source_map::SourceContext,
    project_root: &std::path::Path,
) {
    for item in items.iter_mut() {
        if let Some(href) = item.href.as_ref() {
            let resolved =
                resolve_metadata_path(href, &item.href_source, source_context, project_root);
            item.href = Some(resolved);
        }
        if !item.menu.is_empty() {
            resolve_item_hrefs(&mut item.menu, source_context, project_root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::format::Format;
    use crate::project::index::ProjectIndex;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        }
    }

    fn config_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::for_test())
    }

    fn bool_value(b: bool) -> ConfigValue {
        ConfigValue::new_bool(b, SourceInfo::for_test())
    }

    fn str_value(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::for_test())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    fn profile(source: &str, title: &str) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: source.replace(".qmd", ".html"),
            format_id: "html".to_string(),
            title: Some(title.to_string()),
            ..DocumentProfile::default()
        }
    }

    async fn run_transform(meta: ConfigValue) -> ConfigValue {
        run_transform_with(meta, None).await
    }

    async fn run_transform_with(
        meta: ConfigValue,
        index: Option<Arc<ProjectIndex>>,
    ) -> ConfigValue {
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        if let Some(idx) = index {
            ctx = ctx.with_project_index(idx);
        }
        FooterGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast.meta
    }

    // --- Existing Phase 2 tests -----------------------------------

    #[tokio::test]
    async fn skips_when_absent() {
        let meta = run_transform(ConfigValue::default()).await;
        assert!(!meta.contains_path(&["navigation", "footer"]));
    }

    #[tokio::test]
    async fn skips_when_false() {
        let meta = run_transform(config_map(vec![("page-footer", bool_value(false))])).await;
        assert!(!meta.contains_path(&["navigation", "footer"]));
    }

    #[tokio::test]
    async fn string_shortcut_populates_center() {
        let meta = run_transform(config_map(vec![(
            "page-footer",
            str_value("Copyright 2026"),
        )]))
        .await;
        assert!(meta.contains_path(&["navigation", "footer"]));
        let footer = meta.get_path(&["navigation", "footer"]).unwrap();
        assert_eq!(
            footer
                .get("center")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Copyright 2026")
        );
    }

    #[tokio::test]
    async fn object_form_populates_regions() {
        let footer_cv = config_map(vec![
            ("left", str_value("© 2026")),
            ("background", str_value("light")),
        ]);
        let meta = run_transform(config_map(vec![("page-footer", footer_cv)])).await;
        let stored = meta.get_path(&["navigation", "footer"]).unwrap();
        assert_eq!(
            stored
                .get("left")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("© 2026")
        );
        assert_eq!(
            stored
                .get("background")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("light")
        );
    }

    #[tokio::test]
    async fn skips_when_navigation_footer_already_set() {
        let pre = config_map(vec![("left", str_value("Pre-existing"))]);
        let mut meta = config_map(vec![(
            "page-footer",
            config_map(vec![("left", str_value("Fresh"))]),
        )]);
        meta.insert_path(&["navigation", "footer"], pre);
        let out = run_transform(meta).await;
        let stored = out.get_path(&["navigation", "footer"]).unwrap();
        assert_eq!(
            stored
                .get("left")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Pre-existing")
        );
    }

    // --- Phase 3 enrichment tests ---------------------------------

    /// Defect 2 (bd-page-footer-items-f4th80mj) — Q1's rule, measured:
    /// a bare footer scalar that resolves to a project document stays a
    /// link with that document's title; anything else becomes display
    /// text with no href.
    #[tokio::test]
    async fn footer_generate_demotes_unresolved_bare_items_to_text() {
        let footer_cv = config_map(vec![(
            "left",
            arr(vec![
                str_value("about.qmd"),               // resolves → link
                str_value("Copyright 2026 Example."), // no match → text
                str_value("https://example.com"),     // external → text
            ]),
        )]);
        let meta = config_map(vec![("page-footer", footer_cv)]);
        let index = Arc::new(ProjectIndex::new(vec![profile("about.qmd", "About")]));
        let out = run_transform_with(meta, Some(index)).await;
        let stored = out.get_path(&["navigation", "footer"]).unwrap();
        let left = stored.get("left").and_then(|v| v.as_array()).unwrap();

        assert_eq!(
            left[0]
                .get("text")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("About"),
            "resolving bare item keeps its link and gains a title; got {:?}",
            left[0]
        );
        assert!(
            left[0].get("href").is_some(),
            "resolving bare item keeps its href; got {:?}",
            left[0]
        );

        for (idx, expected) in [
            (1usize, "Copyright 2026 Example."),
            (2, "https://example.com"),
        ] {
            assert_eq!(
                left[idx]
                    .get("text")
                    .and_then(|v| v.as_plain_text())
                    .as_deref(),
                Some(expected),
                "unresolved bare item {} should become text; got {:?}",
                idx,
                left[idx]
            );
            assert!(
                left[idx].get("href").is_none(),
                "unresolved bare item {} must drop its href; got {:?}",
                idx,
                left[idx]
            );
        }
    }

    /// An *explicit* `href:` that matches no document keeps its anchor —
    /// only bare scalars are eligible for demotion.
    #[tokio::test]
    async fn footer_generate_keeps_explicit_href_without_index_match() {
        let item = config_map(vec![("href", str_value("https://example.com/support"))]);
        let footer_cv = config_map(vec![("right", arr(vec![item]))]);
        let meta = config_map(vec![("page-footer", footer_cv)]);
        let index = Arc::new(ProjectIndex::new(vec![profile("about.qmd", "About")]));
        let out = run_transform_with(meta, Some(index)).await;
        let stored = out.get_path(&["navigation", "footer"]).unwrap();
        let right = stored.get("right").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            right[0]
                .get("href")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("https://example.com/support"),
            "explicit href must survive; got {:?}",
            right[0]
        );
    }

    /// Phase 3 test 37 — bare-href items in a footer Items region
    /// get `text` from the matching profile.
    #[tokio::test]
    async fn footer_generate_enriches_items_in_regions() {
        let footer_cv = config_map(vec![("right", arr(vec![str_value("about.qmd")]))]);
        let meta = config_map(vec![("page-footer", footer_cv)]);
        let index = Arc::new(ProjectIndex::new(vec![profile("about.qmd", "About")]));
        let out = run_transform_with(meta, Some(index)).await;
        let stored = out.get_path(&["navigation", "footer"]).unwrap();
        let right = stored.get("right").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            right[0]
                .get("text")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("About"),
            "bare-href footer item should be enriched; got: {:?}",
            right[0]
        );
    }

    /// Phase 3 test 38 — a string-valued region (Text) is not
    /// scanned for .qmd links. It survives exactly as authored.
    #[tokio::test]
    async fn footer_generate_does_not_enrich_text_regions() {
        let footer_cv = config_map(vec![("center", str_value("See [our docs](docs.qmd)"))]);
        let meta = config_map(vec![("page-footer", footer_cv)]);
        let index = Arc::new(ProjectIndex::new(vec![profile("docs.qmd", "Docs")]));
        let out = run_transform_with(meta, Some(index)).await;
        let stored = out.get_path(&["navigation", "footer"]).unwrap();
        assert_eq!(
            stored
                .get("center")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("See [our docs](docs.qmd)"),
            "text region must not be rewritten (Phase 6's concern)"
        );
    }

    /// Phase 3 test 39 — hrefs survive as source paths; the .qmd →
    /// .html rewrite is deferred to Render (format-agnostic invariant).
    #[tokio::test]
    async fn footer_generate_keeps_qmd_paths() {
        let footer_cv = config_map(vec![("right", arr(vec![str_value("about.qmd")]))]);
        let meta = config_map(vec![("page-footer", footer_cv)]);
        let index = Arc::new(ProjectIndex::new(vec![profile("about.qmd", "About")]));
        let out = run_transform_with(meta, Some(index)).await;
        let stored = out.get_path(&["navigation", "footer"]).unwrap();
        let right = stored.get("right").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            right[0]
                .get("href")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("about.qmd"),
            "href should still be the source path at Generate time"
        );
    }

    /// Phase 3 test 40 — standalone render (no ProjectIndex): no
    /// enrichment, footer stored verbatim.
    #[tokio::test]
    async fn footer_generate_no_index_passes_through_unchanged() {
        let footer_cv = config_map(vec![("right", arr(vec![str_value("about.qmd")]))]);
        let meta = config_map(vec![("page-footer", footer_cv)]);
        let out = run_transform_with(meta, None).await;
        let stored = out.get_path(&["navigation", "footer"]).unwrap();
        let right = stored.get("right").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            right[0]
                .get("href")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("about.qmd")
        );
        assert!(
            right[0].get("text").is_none(),
            "no enrichment without index"
        );
    }
}

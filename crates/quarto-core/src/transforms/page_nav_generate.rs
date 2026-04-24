/*
 * page_nav_generate.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Page-navigation (prev/next) Generate transform.
//!
//! Reads the already-resolved sidebar from `navigation.sidebar`
//! (populated upstream by [`SidebarGenerateTransform`](super::SidebarGenerateTransform)),
//! flattens it depth-first per
//! [`flatten_for_page_nav`](quarto_navigation::sidebar::flatten_for_page_nav),
//! finds the current page in the flat list, and stores the resulting
//! [`PageNavigation`] (prev/next neighbors) at
//! `navigation.page_navigation`. Hrefs remain in source-path space —
//! the HTML rewrite happens in
//! [`PageNavRenderTransform`](super::PageNavRenderTransform).
//!
//! See `claude-notes/plans/2026-04-24-websites-phase-4.md` §Decision 4
//! for the flatten algorithm and §Decision 5 for the skip conditions.
//!
//! ## Skip conditions
//!
//! - `page-navigation: false` at the document level (affirmative
//!   disable; flows through metadata merge).
//! - `navigation.page_navigation` already populated — user override.
//! - `navigation.sidebar` absent — no sidebar to flatten.
//! - The current page's source path doesn't appear in the flat list.
//! - Both neighbors are `None` (lonely page).

use quarto_navigation::page_nav::PageNavigation;
use quarto_navigation::sidebar::{FlatEntry, Sidebar, flatten_for_page_nav};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;
use crate::transforms::navigation_active::page_relative_source;

pub struct PageNavGenerateTransform;

impl PageNavGenerateTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PageNavGenerateTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for PageNavGenerateTransform {
    fn name(&self) -> &str {
        "page-nav-generate"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "page-navigation") {
            return Ok(());
        }
        if ast.meta.contains_path(&["navigation", "page_navigation"]) {
            return Ok(());
        }

        let Some(sidebar_cv) = ast.meta.get_path(&["navigation", "sidebar"]) else {
            return Ok(());
        };

        let sidebar = Sidebar::from_config_value(sidebar_cv);
        let flat = flatten_for_page_nav(&sidebar.contents);
        if flat.is_empty() {
            return Ok(());
        }

        let page_source = page_relative_source(ctx);
        let Some(idx) = flat.iter().position(|e| e.is_link_with_href(&page_source)) else {
            // Current page isn't a navigable entry in this sidebar.
            // Absence is fine — no prev/next strip on this page.
            return Ok(());
        };

        let prev = neighbor_before(&flat, idx);
        let next = neighbor_after(&flat, idx);

        if prev.is_none() && next.is_none() {
            // Lonely page — nothing to render.
            return Ok(());
        }

        let page_nav = PageNavigation { prev, next };
        ast.meta.insert_path(
            &["navigation", "page_navigation"],
            page_nav.to_config_value(),
        );

        Ok(())
    }
}

/// Return the entry immediately before `idx` if it is an `Item`. A
/// `Separator` at `idx-1` is a hard boundary (returns `None`).
fn neighbor_before(flat: &[FlatEntry], idx: usize) -> Option<quarto_navigation::NavigationItem> {
    if idx == 0 {
        return None;
    }
    match &flat[idx - 1] {
        FlatEntry::Item(item) => Some(item.clone()),
        FlatEntry::Separator => None,
    }
}

/// Return the entry immediately after `idx` if it is an `Item`. A
/// `Separator` at `idx+1` is a hard boundary (returns `None`).
fn neighbor_after(flat: &[FlatEntry], idx: usize) -> Option<quarto_navigation::NavigationItem> {
    if idx + 1 >= flat.len() {
        return None;
    }
    match &flat[idx + 1] {
        FlatEntry::Item(item) => Some(item.clone()),
        FlatEntry::Separator => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::index::ProjectIndex;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_navigation::Sidebar as NavSidebar;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn config_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::default(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::default())
    }

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::default())
    }

    fn b(x: bool) -> ConfigValue {
        ConfigValue::new_bool(x, SourceInfo::default())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::default())
    }

    fn make_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path("/project/about.qmd")],
            output_dir: PathBuf::from("/project/_site"),
        }
    }

    /// Run the transform with the given meta + page; returns the
    /// resulting `ast.meta`.
    async fn run(meta: ConfigValue, page: &str) -> ConfigValue {
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_project();
        let doc = DocumentInfo::from_path(format!("/project/{}", page));
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let _ = Arc::<ProjectIndex>::default; // silence the import-unused warning if this fn is unused below
        PageNavGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast.meta
    }

    /// Build a sidebar `ConfigValue` from a list of bare-string hrefs
    /// (treated as Link entries) for use as `navigation.sidebar`.
    fn sidebar_with_links(hrefs: &[&str]) -> ConfigValue {
        let entries: Vec<ConfigValue> = hrefs.iter().map(|h| s(h)).collect();
        let sb = NavSidebar {
            contents: quarto_navigation::sidebar::Sidebar::from_config_value(&config_map(vec![(
                "contents",
                arr(entries),
            )]))
            .contents,
            ..NavSidebar::with_defaults()
        };
        sb.to_config_value()
    }

    /// Test 19 — `page-navigation: false` at doc level skips.
    #[tokio::test]
    async fn page_nav_generate_skips_when_feature_disabled() {
        let mut meta = config_map(vec![("page-navigation", b(false))]);
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["a.qmd", "b.qmd"]),
        );
        let out = run(meta, "a.qmd").await;
        assert!(!out.contains_path(&["navigation", "page_navigation"]));
    }

    /// Test 20 — no `navigation.sidebar`: skip silently.
    #[tokio::test]
    async fn page_nav_generate_skips_when_sidebar_absent() {
        let meta = config_map(vec![]);
        let out = run(meta, "a.qmd").await;
        assert!(!out.contains_path(&["navigation", "page_navigation"]));
    }

    /// Test 21 — pre-populated `navigation.page_navigation` survives.
    #[tokio::test]
    async fn page_nav_generate_skips_when_already_populated() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["a.qmd", "b.qmd"]),
        );
        meta.insert_path(
            &["navigation", "page_navigation"],
            config_map(vec![("prev", config_map(vec![("href", s("PRE"))]))]),
        );
        let out = run(meta, "a.qmd").await;
        let stored = out
            .get_path(&["navigation", "page_navigation"])
            .expect("override survives");
        assert_eq!(
            stored
                .get("prev")
                .and_then(|v| v.get("href"))
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("PRE"),
            "user-supplied page_navigation must win"
        );
    }

    /// Test 22 — current page not in sidebar's flat list: no
    /// insertion.
    #[tokio::test]
    async fn page_nav_generate_skips_when_page_not_in_sidebar() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["a.qmd", "b.qmd"]),
        );
        let out = run(meta, "elsewhere.qmd").await;
        assert!(!out.contains_path(&["navigation", "page_navigation"]));
    }

    /// Test 23 — single-page sidebar: lonely page, no insertion.
    #[tokio::test]
    async fn page_nav_generate_skips_when_lonely_page() {
        let mut meta = config_map(vec![]);
        meta.insert_path(&["navigation", "sidebar"], sidebar_with_links(&["a.qmd"]));
        let out = run(meta, "a.qmd").await;
        assert!(!out.contains_path(&["navigation", "page_navigation"]));
    }

    /// Test 24 — middle page has both neighbors.
    #[tokio::test]
    async fn page_nav_generate_middle_page_has_both_neighbors() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["a.qmd", "b.qmd", "c.qmd"]),
        );
        let out = run(meta, "b.qmd").await;
        let stored = out.get_path(&["navigation", "page_navigation"]).unwrap();
        assert_eq!(
            stored
                .get("prev")
                .and_then(|v| v.get("href"))
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("a.qmd")
        );
        assert_eq!(
            stored
                .get("next")
                .and_then(|v| v.get("href"))
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("c.qmd")
        );
    }

    /// Test 25 — first page has only `next`.
    #[tokio::test]
    async fn page_nav_generate_first_page_only_has_next() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["a.qmd", "b.qmd", "c.qmd"]),
        );
        let out = run(meta, "a.qmd").await;
        let stored = out.get_path(&["navigation", "page_navigation"]).unwrap();
        assert!(
            stored.get("prev").is_none(),
            "first page: prev should be None"
        );
        assert_eq!(
            stored
                .get("next")
                .and_then(|v| v.get("href"))
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("b.qmd")
        );
    }

    /// Test 26 — last page has only `prev`.
    #[tokio::test]
    async fn page_nav_generate_last_page_only_has_prev() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["a.qmd", "b.qmd", "c.qmd"]),
        );
        let out = run(meta, "c.qmd").await;
        let stored = out.get_path(&["navigation", "page_navigation"]).unwrap();
        assert_eq!(
            stored
                .get("prev")
                .and_then(|v| v.get("href"))
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("b.qmd")
        );
        assert!(
            stored.get("next").is_none(),
            "last page: next should be None"
        );
    }

    /// Test 27 — separators break adjacency in both directions.
    #[tokio::test]
    async fn page_nav_generate_separator_breaks_adjacency() {
        // Sidebar: [a.qmd, ---, b.qmd]
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["a.qmd", "---", "b.qmd"]),
        );
        let out = run(meta.clone(), "a.qmd").await;
        // a is the only page that survives — but b is also navigable,
        // so the page is *not* lonely. Separator-as-next should yield
        // a no-insertion result for `a`.
        let stored = out.get_path(&["navigation", "page_navigation"]);
        assert!(
            stored.is_none(),
            "a.qmd: separator is the next slot, so neighbor is None; \
             with no prev either, the transform skips. Got: {:?}",
            stored
        );

        let out_b = run(meta, "b.qmd").await;
        let stored_b = out_b.get_path(&["navigation", "page_navigation"]);
        assert!(
            stored_b.is_none(),
            "b.qmd: separator is the prev slot, no next, so the \
             transform skips. Got: {:?}",
            stored_b
        );
    }

    /// Test 27 (variant) — separator between two adjacent pages,
    /// with a third on the same side: the side away from the
    /// separator still has a neighbor, so the strip is emitted.
    #[tokio::test]
    async fn page_nav_generate_separator_one_side_neighbor_remains() {
        // [a.qmd, ---, b.qmd, c.qmd]
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["a.qmd", "---", "b.qmd", "c.qmd"]),
        );
        let out = run(meta, "b.qmd").await;
        let stored = out.get_path(&["navigation", "page_navigation"]).unwrap();
        // prev: separator (None), next: c.qmd
        assert!(stored.get("prev").is_none(), "separator blocks prev");
        assert_eq!(
            stored
                .get("next")
                .and_then(|v| v.get("href"))
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("c.qmd")
        );
    }

    /// Test 28 — format-agnostic invariant: stored hrefs end in `.qmd`.
    #[tokio::test]
    async fn page_nav_generate_keeps_qmd_hrefs() {
        let mut meta = config_map(vec![]);
        meta.insert_path(
            &["navigation", "sidebar"],
            sidebar_with_links(&["a.qmd", "b.qmd", "c.qmd"]),
        );
        let out = run(meta, "b.qmd").await;
        let stored = out.get_path(&["navigation", "page_navigation"]).unwrap();
        for side in ["prev", "next"] {
            let h = stored
                .get(side)
                .and_then(|v| v.get("href"))
                .and_then(|v| v.as_plain_text())
                .unwrap();
            assert!(
                h.ends_with(".qmd"),
                "{} href must remain .qmd; got {}",
                side,
                h
            );
        }
    }

    /// Test 29 — items resolve from a sidebar whose links carry text
    /// (simulating Phase 2 enrichment): the `text` flows into prev/next.
    #[tokio::test]
    async fn page_nav_generate_carries_enriched_text_from_sidebar() {
        // Sidebar with explicit text: [{href: a.qmd, text: A}, …]
        let entries = arr(vec![
            config_map(vec![("href", s("a.qmd")), ("text", s("Alpha"))]),
            config_map(vec![("href", s("b.qmd")), ("text", s("Bravo"))]),
            config_map(vec![("href", s("c.qmd")), ("text", s("Charlie"))]),
        ]);
        let sidebar_cv =
            quarto_navigation::sidebar::Sidebar::from_config_value(&config_map(vec![(
                "contents", entries,
            )]))
            .to_config_value();
        let mut meta = config_map(vec![]);
        meta.insert_path(&["navigation", "sidebar"], sidebar_cv);
        let out = run(meta, "b.qmd").await;
        let stored = out.get_path(&["navigation", "page_navigation"]).unwrap();
        assert_eq!(
            stored
                .get("prev")
                .and_then(|v| v.get("text"))
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Alpha")
        );
        assert_eq!(
            stored
                .get("next")
                .and_then(|v| v.get("text"))
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Charlie")
        );
    }

    /// Test 30 — section header with an href can be a neighbor.
    #[tokio::test]
    async fn page_nav_generate_respects_section_header_as_neighbor() {
        // Sidebar:
        //   - index.qmd
        //   - section: Docs
        //     href: docs/index.qmd
        //     contents: [docs/a.qmd]
        let section = config_map(vec![
            ("section", s("Docs")),
            ("href", s("docs/index.qmd")),
            ("contents", arr(vec![s("docs/a.qmd")])),
        ]);
        let sidebar_cv =
            quarto_navigation::sidebar::Sidebar::from_config_value(&config_map(vec![(
                "contents",
                arr(vec![s("index.qmd"), section]),
            )]))
            .to_config_value();
        let mut meta = config_map(vec![]);
        meta.insert_path(&["navigation", "sidebar"], sidebar_cv);
        // Rendering index.qmd: next should be docs/index.qmd (the section header).
        let out = run(meta, "index.qmd").await;
        let stored = out.get_path(&["navigation", "page_navigation"]).unwrap();
        assert_eq!(
            stored
                .get("next")
                .and_then(|v| v.get("href"))
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("docs/index.qmd")
        );
    }
}

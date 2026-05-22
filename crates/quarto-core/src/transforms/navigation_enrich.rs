/*
 * navigation_enrich.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Text enrichment for navbar / sidebar / page-footer items.
//!
//! When an author writes a bare-path navigation item —
//! `- about.qmd` in YAML — they haven't told us what label to show.
//! `NavigationItem::from_config_value` parses it as `href:
//! "about.qmd"`, `text: None`. This helper walks a slice of items
//! and, when there's a `ProjectIndex` lookup hit, fills in `text`
//! from the referenced document's `DocumentProfile.title`.
//!
//! The enrichment is **format-agnostic**: only `text` is touched,
//! never `href`. The `.qmd → .html` rewrite remains a Render concern
//! (see [`super::navigation_href`]). Recursion into `menu` gives
//! dropdown items the same treatment.
//!
//! Factored out of `sidebar_generate.rs` during Phase 3 so navbar +
//! page-footer can share the same enrichment rule. See
//! `claude-notes/plans/2026-04-24-websites-phase-3.md` §Decision 4.

use quarto_navigation::NavigationItem;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::{By, SourceInfo};
use std::path::Path;

use crate::project::index::ProjectIndex;

/// Walk `items`, filling in `text` on any entry that has an `href`
/// matching a profile in the index but no author-supplied `text`.
/// Recurses into `menu` for dropdown items.
///
/// Never modifies `href`; never modifies items that already have a
/// `text`. External URLs never enrich (by construction — no index
/// lookup will hit).
pub fn enrich_navigation_items(items: &mut [NavigationItem], index: &ProjectIndex) {
    for item in items.iter_mut() {
        enrich_one(item, index);
        if !item.menu.is_empty() {
            enrich_navigation_items(&mut item.menu, index);
        }
    }
}

/// Enrich a single item's `text` from the index when possible.
/// Public-within-crate so callers that already own a `NavigationItem`
/// (e.g. sidebar's `Link` wrapper) can delegate without re-slicing.
pub(crate) fn enrich_one(item: &mut NavigationItem, index: &ProjectIndex) {
    if item.text.is_some() {
        return;
    }
    let Some(href) = item.href.as_deref() else {
        return;
    };
    if let Some(profile) = index.lookup_by_source(Path::new(href)) {
        if let Some(title) = &profile.title {
            item.text = Some(ConfigValue::new_string(
                title,
                SourceInfo::generated(By::programmatic_config()),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use std::path::PathBuf;

    fn profile(source: &str, title: &str) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: source.replace(".qmd", ".html"),
            format_id: "html".to_string(),
            title: Some(title.to_string()),
            ..DocumentProfile::default()
        }
    }

    /// Phase 3 test 15 — bare-href item gets text from matching profile.
    #[test]
    fn enrich_fills_missing_text_from_profile_title() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "About Us")]);
        let mut items = vec![NavigationItem {
            href: Some("about.qmd".to_string()),
            ..NavigationItem::default()
        }];
        enrich_navigation_items(&mut items, &idx);
        assert_eq!(
            items[0]
                .text
                .as_ref()
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("About Us")
        );
    }

    /// Phase 3 test 16 — existing text is not clobbered.
    #[test]
    fn enrich_does_not_clobber_explicit_text() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "About Us")]);
        let mut items = vec![NavigationItem {
            href: Some("about.qmd".to_string()),
            text: Some(ConfigValue::new_string("Profile", SourceInfo::for_test())),
            ..NavigationItem::default()
        }];
        enrich_navigation_items(&mut items, &idx);
        assert_eq!(
            items[0]
                .text
                .as_ref()
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Profile"),
            "author-supplied text should survive"
        );
    }

    /// Phase 3 test 17 — enrichment recurses into dropdown menus.
    #[test]
    fn enrich_recurses_into_menu() {
        let idx = ProjectIndex::new(vec![
            profile("start.qmd", "Getting Started"),
            profile("ref.qmd", "Reference"),
        ]);
        let mut items = vec![NavigationItem {
            text: Some(ConfigValue::new_string("Docs", SourceInfo::for_test())),
            menu: vec![
                NavigationItem {
                    href: Some("start.qmd".to_string()),
                    ..NavigationItem::default()
                },
                NavigationItem {
                    href: Some("ref.qmd".to_string()),
                    ..NavigationItem::default()
                },
            ],
            ..NavigationItem::default()
        }];
        enrich_navigation_items(&mut items, &idx);
        assert_eq!(
            items[0].menu[0]
                .text
                .as_ref()
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Getting Started")
        );
        assert_eq!(
            items[0].menu[1]
                .text
                .as_ref()
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Reference")
        );
    }

    /// Phase 3 test 18 — external URLs don't match the index; text
    /// stays `None`.
    #[test]
    fn enrich_skips_external_urls() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "About")]);
        let mut items = vec![NavigationItem {
            href: Some("https://example.com".to_string()),
            ..NavigationItem::default()
        }];
        enrich_navigation_items(&mut items, &idx);
        assert!(items[0].text.is_none());
    }

    /// Items without an `href` (pure submenus, `text`-only headings)
    /// are unaffected.
    #[test]
    fn enrich_noop_for_item_without_href() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "About")]);
        let mut items = vec![NavigationItem {
            text: Some(ConfigValue::new_string("Header", SourceInfo::for_test())),
            ..NavigationItem::default()
        }];
        enrich_navigation_items(&mut items, &idx);
        assert_eq!(
            items[0]
                .text
                .as_ref()
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Header")
        );
    }

    /// An item with an href that doesn't hit the index is unchanged
    /// (no warning, no enrichment).
    #[test]
    fn enrich_noop_when_index_miss() {
        let idx = ProjectIndex::new(vec![]);
        let mut items = vec![NavigationItem {
            href: Some("missing.qmd".to_string()),
            ..NavigationItem::default()
        }];
        enrich_navigation_items(&mut items, &idx);
        assert!(items[0].text.is_none());
        assert_eq!(items[0].href.as_deref(), Some("missing.qmd"));
    }
}

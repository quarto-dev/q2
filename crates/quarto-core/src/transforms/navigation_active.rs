/*
 * navigation_active.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Active-item marking for flat navbar / footer item lists.
//!
//! Walks a `&mut [NavigationItem]` and marks any entry whose `href`
//! (interpreted as a project-relative source path) equals the current
//! page's source path with `item.active = true`. Recurses into
//! dropdown `menu` children.
//!
//! Format-agnostic: compares source paths only, no HTML assumptions,
//! no index lookup (the caller has already determined the current
//! page's source path). Matches Phase 2's sidebar active-marking
//! contract — see
//! `claude-notes/plans/2026-04-24-websites-phase-3.md` §Decision 5.
//!
//! Unlike sidebars, navbar/footer items don't have an "expand
//! ancestors" semantic. A matched item in a dropdown menu marks
//! *only* itself active; the top-level dropdown remains inactive.
//! This matches Q1's navbar behavior.

use quarto_navigation::NavigationItem;

use crate::render::RenderContext;
use crate::transforms::navigation_href::is_external;

/// Project-relative source path for the current page, forward-slash
/// normalized. Falls back to the basename if the input isn't under
/// the project root (shouldn't happen in practice, but defensive).
///
/// Shared across sidebar / navbar / footer Generate transforms — each
/// needs the same "who am I?" answer to pass to the active-marking
/// pass or to the sidebar-for-page helper.
pub fn page_relative_source(ctx: &RenderContext) -> String {
    let relative = ctx
        .document
        .input
        .strip_prefix(&ctx.project.dir)
        .unwrap_or_else(|_| {
            ctx.document
                .input
                .file_name()
                .map(std::path::Path::new)
                .unwrap_or(&ctx.document.input)
        });
    relative.to_string_lossy().replace('\\', "/")
}

/// Mark the first `NavigationItem` (flat or nested under `menu`)
/// whose `href` matches `page_source` as active. Returns `true` when
/// a match was found.
///
/// Comparison is exact-string on source paths. External URLs and
/// fragment anchors never match; items without an `href` never
/// match. Runs over the entire slice even after a hit, so if a user
/// configures the same page in two places both become active
/// (that's a legitimate case — e.g. "Home" in both left and right).
pub fn mark_active(items: &mut [NavigationItem], page_source: &str) -> bool {
    let mut any = false;
    for item in items.iter_mut() {
        if let Some(href) = item.href.as_deref() {
            if !is_external(href) && !href.starts_with('#') && href == page_source {
                item.active = true;
                any = true;
            }
        }
        if !item.menu.is_empty() && mark_active(&mut item.menu, page_source) {
            any = true;
        }
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_source_map::SourceInfo;

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::for_test())
    }

    /// Phase 3 test 19 — item whose href matches `page_source` becomes
    /// active.
    #[test]
    fn mark_active_matches_by_source_path() {
        let mut items = vec![
            NavigationItem {
                href: Some("index.qmd".to_string()),
                text: Some(s("Home")),
                ..NavigationItem::default()
            },
            NavigationItem {
                href: Some("about.qmd".to_string()),
                text: Some(s("About")),
                ..NavigationItem::default()
            },
        ];
        let matched = mark_active(&mut items, "about.qmd");
        assert!(matched);
        assert!(!items[0].active);
        assert!(items[1].active);
    }

    /// Phase 3 test 20 — non-matching page source leaves items alone.
    #[test]
    fn mark_active_does_not_match_other_pages() {
        let mut items = vec![
            NavigationItem {
                href: Some("index.qmd".to_string()),
                ..NavigationItem::default()
            },
            NavigationItem {
                href: Some("about.qmd".to_string()),
                ..NavigationItem::default()
            },
        ];
        let matched = mark_active(&mut items, "docs/api.qmd");
        assert!(!matched);
        assert!(!items[0].active);
        assert!(!items[1].active);
    }

    /// Phase 3 test 21 — enrichment recurses into `menu`. A leaf
    /// inside a dropdown matches; the dropdown's own anchor stays
    /// inactive.
    #[test]
    fn mark_active_recurses_into_menu() {
        let mut items = vec![NavigationItem {
            text: Some(s("Docs")),
            menu: vec![
                NavigationItem {
                    href: Some("docs/intro.qmd".to_string()),
                    ..NavigationItem::default()
                },
                NavigationItem {
                    href: Some("docs/advanced.qmd".to_string()),
                    ..NavigationItem::default()
                },
            ],
            ..NavigationItem::default()
        }];
        let matched = mark_active(&mut items, "docs/advanced.qmd");
        assert!(matched);
        // Dropdown itself stays inactive — Phase 3 decision 5.
        assert!(!items[0].active);
        assert!(!items[0].menu[0].active);
        assert!(items[0].menu[1].active);
    }

    /// Phase 3 test 22 — external URLs never match.
    #[test]
    fn mark_active_skips_external_urls() {
        let mut items = vec![
            NavigationItem {
                href: Some("https://example.com".to_string()),
                ..NavigationItem::default()
            },
            NavigationItem {
                href: Some("mailto:a@b.c".to_string()),
                ..NavigationItem::default()
            },
            NavigationItem {
                href: Some("#section".to_string()),
                ..NavigationItem::default()
            },
        ];
        let matched = mark_active(&mut items, "https://example.com");
        assert!(!matched, "external href string-equality should not count");
        assert!(items.iter().all(|i| !i.active));
    }

    /// Items without an `href` (pure submenus, plain headings) are
    /// skipped for the active check but still descended into for
    /// their menu children.
    #[test]
    fn mark_active_ignores_hrefless_items_but_descends_into_menu() {
        let mut items = vec![NavigationItem {
            text: Some(s("Docs")),
            // No href.
            menu: vec![NavigationItem {
                href: Some("guide.qmd".to_string()),
                ..NavigationItem::default()
            }],
            ..NavigationItem::default()
        }];
        let matched = mark_active(&mut items, "guide.qmd");
        assert!(matched);
        assert!(items[0].menu[0].active);
    }

    /// Duplicate matches (same href in multiple places) both go
    /// active. This is intentional — if a user puts "Home" in the
    /// navbar twice, both should highlight when on home.
    #[test]
    fn mark_active_marks_all_matching_items() {
        let mut items = vec![
            NavigationItem {
                href: Some("home.qmd".to_string()),
                ..NavigationItem::default()
            },
            NavigationItem {
                href: Some("about.qmd".to_string()),
                ..NavigationItem::default()
            },
            NavigationItem {
                href: Some("home.qmd".to_string()),
                ..NavigationItem::default()
            },
        ];
        mark_active(&mut items, "home.qmd");
        assert!(items[0].active);
        assert!(!items[1].active);
        assert!(items[2].active);
    }
}

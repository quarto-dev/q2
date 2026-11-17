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
//!
//! [`mark_active_for_section`] is the navbar's fallback for pages the
//! navbar doesn't link to directly: the first top-level item whose
//! `href` also appears in the page's sidebar goes active, so readers
//! see which section of the site they are in (GH #1064).

use std::collections::HashSet;

use quarto_navigation::{NavigationItem, Sidebar, SidebarEntry};

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
    page_relative_source_of(&ctx.document.input, &ctx.project.dir)
}

/// [`page_relative_source`] for callers that hold the document path and
/// the project directory but no `RenderContext` (pipeline stages).
pub fn page_relative_source_of(input: &std::path::Path, project_dir: &std::path::Path) -> String {
    let relative = input
        .strip_prefix(project_dir)
        .unwrap_or_else(|_| input.file_name().map_or(input, std::path::Path::new));
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
        if let Some(href) = item.href.as_deref()
            && !is_external(href)
            && !href.starts_with('#')
            && href == page_source
        {
            item.active = true;
            any = true;
        }
        if !item.menu.is_empty() && mark_active(&mut item.menu, page_source) {
            any = true;
        }
    }
    any
}

/// Every `href` in `sidebar` (links and section headers, recursively).
pub fn sidebar_hrefs(sidebar: &Sidebar) -> HashSet<String> {
    fn walk(entries: &[SidebarEntry], out: &mut HashSet<String>) {
        for entry in entries {
            match entry {
                SidebarEntry::Link { item } => out.extend(item.href.clone()),
                SidebarEntry::Section { href, contents, .. } => {
                    out.extend(href.clone());
                    walk(contents, out);
                }
                SidebarEntry::Separator | SidebarEntry::Heading(_) | SidebarEntry::Auto(_) => {}
            }
        }
    }
    let mut out = HashSet::new();
    walk(&sidebar.contents, &mut out);
    out
}

/// Mark the first top-level item (in `groups` order) whose `href` is
/// one of `section_hrefs` as active, and stop. Returns `true` on a hit.
///
/// Q1 parity: its website post-processor marks the first `a.nav-link`
/// whose href is in the page's `.sidebar-navigation`. Dropdown
/// children render as `dropdown-item`, not `nav-link`, so they are not
/// considered. Callers run this only when [`mark_active`] found no
/// exact match, so a page's own navbar link always wins.
pub fn mark_active_for_section(
    groups: &mut [&mut [NavigationItem]],
    section_hrefs: &HashSet<String>,
) -> bool {
    for items in groups.iter_mut() {
        for item in items.iter_mut() {
            if let Some(href) = item.href.as_deref()
                && !is_external(href)
                && !href.starts_with('#')
                && section_hrefs.contains(href)
            {
                item.active = true;
                return true;
            }
        }
    }
    false
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

    fn link(href: &str) -> SidebarEntry {
        SidebarEntry::Link {
            item: NavigationItem {
                href: Some(href.to_string()),
                ..NavigationItem::default()
            },
        }
    }

    /// GH #1064 — section hrefs come from links and section headers at
    /// any depth.
    #[test]
    fn sidebar_hrefs_collects_links_and_section_headers() {
        let mut sidebar = Sidebar::with_defaults();
        sidebar.contents = vec![
            link("guide/index.qmd"),
            SidebarEntry::Section {
                text: None,
                href: Some("guide/advanced/index.qmd".to_string()),
                href_source: SourceInfo::for_test(),
                id: None,
                contents: vec![link("guide/advanced/tuning.qmd")],
                expanded: false,
                active: false,
            },
            SidebarEntry::Separator,
        ];
        let hrefs = sidebar_hrefs(&sidebar);
        let mut got: Vec<_> = hrefs.iter().map(String::as_str).collect();
        got.sort_unstable();
        assert_eq!(
            got,
            vec![
                "guide/advanced/index.qmd",
                "guide/advanced/tuning.qmd",
                "guide/index.qmd"
            ]
        );
    }

    /// GH #1064 — only the first top-level item whose href is in the
    /// section goes active, searching `left` before `right`; dropdown
    /// children and external links never match.
    #[test]
    fn mark_active_for_section_marks_first_top_level_match() {
        let mut left = [
            NavigationItem {
                href: Some("index.qmd".to_string()),
                ..NavigationItem::default()
            },
            NavigationItem {
                text: Some(s("Menu")),
                menu: vec![NavigationItem {
                    href: Some("guide/install.qmd".to_string()),
                    ..NavigationItem::default()
                }],
                ..NavigationItem::default()
            },
        ];
        let mut right = [
            NavigationItem {
                href: Some("guide/index.qmd".to_string()),
                ..NavigationItem::default()
            },
            NavigationItem {
                href: Some("guide/install.qmd".to_string()),
                ..NavigationItem::default()
            },
        ];
        let section: HashSet<String> = ["guide/index.qmd", "guide/install.qmd"]
            .into_iter()
            .map(String::from)
            .collect();
        let matched = mark_active_for_section(&mut [&mut left[..], &mut right[..]], &section);
        assert!(matched);
        assert!(!left[0].active);
        assert!(
            !left[1].menu[0].active,
            "dropdown children are not nav-links"
        );
        assert!(right[0].active);
        assert!(!right[1].active, "only the first match goes active");
    }

    #[test]
    fn mark_active_for_section_no_match() {
        let mut left = [NavigationItem {
            href: Some("https://example.com".to_string()),
            ..NavigationItem::default()
        }];
        let section: HashSet<String> = ["https://example.com".to_string()].into();
        assert!(!mark_active_for_section(&mut [&mut left[..]], &section));
        assert!(!left[0].active);
    }
}

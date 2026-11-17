/*
 * navigation_drafts.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Drop navigation entries that point at unlinkable drafts.
//!
//! Quarto 1's nav templates (`navitem.ejs`, `sidebaritem.ejs`,
//! `nav-footer-navitem.ejs`) skip any item whose target is a draft
//! unless `draft-mode` is `visible`. q2 does the same in the Generate
//! transforms, against the shared policy
//! ([`ProjectIndex::is_linkable`], see [`crate::project::drafts`]), so
//! everything derived from the generated navigation — active state,
//! prev/next page links, breadcrumbs — never sees the draft.
//!
//! Hrefs here are project-root-relative, as the Generate transforms
//! hold them after `resolve_metadata_path`; only hrefs that name a
//! project source in the index can be drafts.
//!
//! One deliberate difference from Q1: a sidebar *section* whose header
//! links to a draft keeps its children and loses only the link. Q1's
//! template drops the whole section, children included, which would
//! hide published pages because their section index is a draft.

use std::path::Path;

use quarto_navigation::{NavigationItem, SidebarEntry};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::{By, SourceInfo};

use crate::project::index::ProjectIndex;
use crate::transforms::navigation_href::is_external;

/// Does `href` name a project page that may not be linked?
pub(crate) fn href_targets_unlinkable(href: &str, index: &ProjectIndex) -> bool {
    if is_external(href) || href.starts_with('#') {
        return false;
    }
    let path = href.split(['#', '?']).next().unwrap_or(href);
    let path = path.strip_prefix('/').unwrap_or(path);
    index
        .lookup_by_source(Path::new(path))
        .is_some_and(|profile| !index.is_linkable(profile))
}

/// Remove navbar / footer items that link to unlinkable drafts,
/// recursing into dropdown menus. A dropdown whose own entries are all
/// removed stays (Q1 parity: the gate is per item).
pub(crate) fn prune_unlinkable_items(items: &mut Vec<NavigationItem>, index: &ProjectIndex) {
    items.retain(|item| {
        !item
            .href
            .as_deref()
            .is_some_and(|href| href_targets_unlinkable(href, index))
    });
    for item in items.iter_mut() {
        if !item.menu.is_empty() {
            prune_unlinkable_items(&mut item.menu, index);
        }
    }
}

/// Remove sidebar links to unlinkable drafts. A section whose header
/// links to one keeps its contents and loses the link; when it had no
/// label of its own it takes the target's title, as an unlabelled
/// section header would have shown it.
pub(crate) fn prune_unlinkable_sidebar(entries: &mut Vec<SidebarEntry>, index: &ProjectIndex) {
    entries.retain(|entry| match entry {
        SidebarEntry::Link { item } => !item
            .href
            .as_deref()
            .is_some_and(|href| href_targets_unlinkable(href, index)),
        _ => true,
    });
    for entry in entries.iter_mut() {
        if let SidebarEntry::Section {
            text,
            href,
            contents,
            ..
        } = entry
        {
            if let Some(target) = href.as_deref()
                && href_targets_unlinkable(target, index)
            {
                if text.is_none() {
                    let path = target.split(['#', '?']).next().unwrap_or(target);
                    *text = index
                        .lookup_by_source(Path::new(path.trim_start_matches('/')))
                        .and_then(|p| p.title_text())
                        .map(|title| {
                            ConfigValue::new_string(
                                &title,
                                SourceInfo::generated(By::programmatic_config()),
                            )
                        });
                }
                *href = None;
            }
            prune_unlinkable_sidebar(contents, index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::project::drafts::{DraftMode, DraftView, RenderAudience};
    use std::path::PathBuf;

    fn profile(source: &str, draft: bool) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: source.replace(".qmd", ".html"),
            format_id: "html".to_string(),
            title: Some(crate::document_profile::text(source.replace(".qmd", ""))),
            draft,
            ..DocumentProfile::default()
        }
    }

    fn index() -> ProjectIndex {
        ProjectIndex::new(vec![profile("a.qmd", false), profile("d.qmd", true)])
    }

    fn item(href: &str) -> NavigationItem {
        NavigationItem {
            href: Some(href.to_string()),
            ..NavigationItem::default()
        }
    }

    fn hrefs(items: &[NavigationItem]) -> Vec<&str> {
        items.iter().filter_map(|i| i.href.as_deref()).collect()
    }

    #[test]
    fn classifies_hrefs() {
        let idx = index();
        assert!(href_targets_unlinkable("d.qmd", &idx));
        assert!(href_targets_unlinkable("/d.qmd#sec", &idx));
        assert!(!href_targets_unlinkable("a.qmd", &idx));
        assert!(!href_targets_unlinkable("missing.qmd", &idx));
        assert!(!href_targets_unlinkable("https://example.com/d.qmd", &idx));
        assert!(!href_targets_unlinkable("#d", &idx));
    }

    #[test]
    fn prunes_items_and_menus() {
        let idx = index();
        let mut items = vec![
            item("a.qmd"),
            item("d.qmd"),
            NavigationItem {
                menu: vec![item("d.qmd"), item("a.qmd")],
                ..NavigationItem::default()
            },
        ];
        prune_unlinkable_items(&mut items, &idx);
        assert_eq!(hrefs(&items), vec!["a.qmd"]);
        assert_eq!(items.len(), 2, "the dropdown itself stays");
        assert_eq!(hrefs(&items[1].menu), vec!["a.qmd"]);
    }

    #[test]
    fn visible_drafts_and_authoring_views_keep_items() {
        let mut visible = profile("d.qmd", true);
        visible.draft_mode = DraftMode::Visible;
        let idx = ProjectIndex::new(vec![visible]);
        let mut items = vec![item("d.qmd")];
        prune_unlinkable_items(&mut items, &idx);
        assert_eq!(items.len(), 1);

        let idx = index().with_draft_view(DraftView::new(RenderAudience::Authoring, true));
        let mut items = vec![item("d.qmd")];
        prune_unlinkable_items(&mut items, &idx);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn sidebar_section_linking_a_draft_keeps_its_children() {
        let idx = index();
        let mut entries = vec![
            SidebarEntry::Link {
                item: item("d.qmd"),
            },
            SidebarEntry::Section {
                text: None,
                href: Some("d.qmd".to_string()),
                href_source: SourceInfo::generated(By::programmatic_config()),
                id: None,
                contents: vec![
                    SidebarEntry::Link {
                        item: item("a.qmd"),
                    },
                    SidebarEntry::Link {
                        item: item("d.qmd"),
                    },
                ],
                expanded: false,
                active: false,
            },
        ];
        prune_unlinkable_sidebar(&mut entries, &idx);
        assert_eq!(entries.len(), 1);
        let SidebarEntry::Section {
            text,
            href,
            contents,
            ..
        } = &entries[0]
        else {
            panic!("expected the section to survive");
        };
        assert_eq!(href, &None);
        assert_eq!(
            text.as_ref().and_then(|t| t.as_str()),
            Some("d"),
            "an unlabelled section keeps the target's title as its label"
        );
        assert_eq!(contents.len(), 1);
    }
}

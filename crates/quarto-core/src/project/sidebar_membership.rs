/*
 * project/sidebar_membership.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Pass-1 sidebar membership resolution.
//!
//! Returns the set of project documents each declared sidebar
//! contains, in declared order, with `auto:` directives expanded
//! against a [`ProjectIndex`]. Used by Phase 8's dependency graph
//! to derive sidebar co-membership edges.
//!
//! This is the **read-only**, side-effect-free counterpart to
//! Phase 2's [`SidebarGenerateTransform`](crate::transforms::sidebar_generate::SidebarGenerateTransform),
//! which produces resolved sidebars *with* enrichment, active-state
//! marking, and per-page picking. See
//! `claude-notes/designs/sidebar-auto-expansion-contract.md` for
//! the prose contract.
//!
//! The two helpers share the same underlying expansion code
//! ([`expand_auto`](crate::transforms::sidebar_auto::expand_auto)),
//! so the dependency-graph view of sidebar membership is consistent
//! with what the rendered sidebar actually displays.

use std::path::PathBuf;

use quarto_config::resolve_website_value;
use quarto_error_reporting::DiagnosticMessage;
use quarto_navigation::{Sidebar, SidebarEntry};
use quarto_pandoc_types::ConfigValue;

use crate::project::index::ProjectIndex;
use crate::transforms::sidebar_auto::expand_auto;

/// One resolved sidebar's membership set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSidebar {
    /// Sidebar id, when the user supplied `id:`. Multi-sidebar
    /// projects use this to disambiguate which sidebar applies to
    /// which page.
    pub id: Option<String>,

    /// Project-relative source paths in declared order. `auto:`
    /// directives are already expanded; section headers contribute
    /// their `href` if any. Pages declared as bare strings or via
    /// nested sections appear here exactly once each (dedup by
    /// path; first occurrence wins).
    pub members: Vec<PathBuf>,
}

/// Walk a project's sidebar configuration and return the membership
/// set per sidebar.
///
/// Reads the merged sidebar config (accepting both top-level
/// `sidebar:` and `website.sidebar:`; see
/// [`quarto_config::resolve_website_value`]) and produces a
/// [`ResolvedSidebar`] for each declared sidebar.
///
/// Diagnostics from `auto:` expansion are appended to
/// `diagnostics`. The function consumes none of `meta`'s ownership
/// and never mutates the index.
///
/// Returns an empty Vec if no sidebars are declared.
pub fn resolve_sidebar_membership(
    meta: &ConfigValue,
    index: &ProjectIndex,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<ResolvedSidebar> {
    let Some(sidebar_cv) = resolve_website_value(meta, "sidebar") else {
        return Vec::new();
    };

    let sidebars = Sidebar::parse_list_from_config(&sidebar_cv);
    sidebars
        .into_iter()
        .map(|mut sidebar| {
            expand_auto(&mut sidebar, index, diagnostics);
            ResolvedSidebar {
                id: sidebar.id.clone(),
                members: collect_member_paths(&sidebar.contents),
            }
        })
        .collect()
}

/// Walk a sidebar's resolved entry tree and collect the project-
/// relative source path of every page that appears, in document
/// order. Skips entries that don't reference a page (separators,
/// pure-text headings, sections with no href).
fn collect_member_paths(entries: &[SidebarEntry]) -> Vec<PathBuf> {
    fn push_unique(out: &mut Vec<PathBuf>, p: PathBuf) {
        if !out.contains(&p) {
            out.push(p);
        }
    }

    fn walk(entries: &[SidebarEntry], out: &mut Vec<PathBuf>) {
        for entry in entries {
            match entry {
                SidebarEntry::Link { item } => {
                    if let Some(href) = item.href.as_deref() {
                        // External URLs and fragment-only anchors
                        // aren't project pages; skip.
                        if !is_external_or_anchor(href) {
                            push_unique(out, PathBuf::from(href));
                        }
                    }
                }
                SidebarEntry::Section { href, contents, .. } => {
                    if let Some(h) = href.as_deref() {
                        if !is_external_or_anchor(h) {
                            push_unique(out, PathBuf::from(h));
                        }
                    }
                    walk(contents, out);
                }
                SidebarEntry::Separator | SidebarEntry::Heading(_) | SidebarEntry::Auto(_) => {}
            }
        }
    }

    let mut out = Vec::new();
    walk(entries, &mut out);
    out
}

/// Cheap classifier: a sidebar entry's `href` that *isn't* a
/// project page. Mirrors the rule used by
/// [`crate::transforms::navigation_href::is_external`] but
/// duplicated here so this helper has zero non-project-internal
/// dependencies.
fn is_external_or_anchor(href: &str) -> bool {
    href.starts_with("http://")
        || href.starts_with("https://")
        || href.starts_with("mailto:")
        || href.starts_with("tel:")
        || href.starts_with("ftp://")
        || href.starts_with("//")
        || href.starts_with('#')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

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

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::default())
    }

    fn make_index_with(paths: &[&str]) -> ProjectIndex {
        let profiles: Vec<DocumentProfile> = paths
            .iter()
            .map(|p| DocumentProfile {
                source_path: PathBuf::from(p),
                output_href: p.replace(".qmd", ".html"),
                format_id: "html".to_string(),
                title: Some(p.to_string()),
                ..DocumentProfile::default()
            })
            .collect();
        ProjectIndex::new(profiles)
    }

    #[test]
    fn empty_meta_returns_no_sidebars() {
        let meta = ConfigValue::default();
        let index = make_index_with(&["a.qmd"]);
        let mut diags = Vec::new();
        assert!(resolve_sidebar_membership(&meta, &index, &mut diags).is_empty());
        assert!(diags.is_empty());
    }

    #[test]
    fn single_sidebar_string_entries() {
        // website.sidebar.contents: [a.qmd, b.qmd]
        let mut meta = ConfigValue::default();
        let sidebar = config_map(vec![("contents", arr(vec![s("a.qmd"), s("b.qmd")]))]);
        meta.insert_path(&["website", "sidebar"], sidebar);

        let index = make_index_with(&["a.qmd", "b.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, None);
        assert_eq!(
            resolved[0].members,
            vec![PathBuf::from("a.qmd"), PathBuf::from("b.qmd")]
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn multi_sidebar_keeps_ids() {
        // website.sidebar: [{id: docs, contents: [a.qmd]}, {id: blog, contents: [b.qmd]}]
        let mut meta = ConfigValue::default();
        let docs = config_map(vec![("id", s("docs")), ("contents", arr(vec![s("a.qmd")]))]);
        let blog = config_map(vec![("id", s("blog")), ("contents", arr(vec![s("b.qmd")]))]);
        meta.insert_path(&["website", "sidebar"], arr(vec![docs, blog]));

        let index = make_index_with(&["a.qmd", "b.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].id.as_deref(), Some("docs"));
        assert_eq!(resolved[0].members, vec![PathBuf::from("a.qmd")]);
        assert_eq!(resolved[1].id.as_deref(), Some("blog"));
        assert_eq!(resolved[1].members, vec![PathBuf::from("b.qmd")]);
    }

    #[test]
    fn auto_directive_expands_against_index() {
        // website.sidebar.contents: [auto: true]
        let mut meta = ConfigValue::default();
        let auto_entry = config_map(vec![(
            "auto",
            ConfigValue::new_bool(true, SourceInfo::default()),
        )]);
        let sidebar = config_map(vec![("contents", arr(vec![auto_entry]))]);
        meta.insert_path(&["website", "sidebar"], sidebar);

        let index = make_index_with(&["a.qmd", "b.qmd", "c.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved.len(), 1);
        // expand_auto enumerates non-draft pages from the index.
        // Order is index order (Pass-1 discovery).
        let names: Vec<&str> = resolved[0]
            .members
            .iter()
            .map(|p| p.to_str().unwrap())
            .collect();
        assert!(names.contains(&"a.qmd"));
        assert!(names.contains(&"b.qmd"));
        assert!(names.contains(&"c.qmd"));
    }

    #[test]
    fn nested_section_recurses() {
        // website.sidebar.contents:
        //   - section: "Group"
        //     contents: [a.qmd, b.qmd]
        let mut meta = ConfigValue::default();
        let section = config_map(vec![
            ("section", s("Group")),
            ("contents", arr(vec![s("a.qmd"), s("b.qmd")])),
        ]);
        let sidebar = config_map(vec![("contents", arr(vec![section]))]);
        meta.insert_path(&["website", "sidebar"], sidebar);

        let index = make_index_with(&["a.qmd", "b.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].members,
            vec![PathBuf::from("a.qmd"), PathBuf::from("b.qmd")]
        );
    }

    // ---- bd-jjep / bd-telo: accept top-level `sidebar:` form too ----

    #[test]
    fn top_level_sidebar_is_picked_up() {
        // Mirror image of the navbar/footer cliff: previously this
        // resolver only read `website.sidebar`, so top-level
        // `sidebar:` was silently ignored.
        let mut meta = ConfigValue::default();
        let sidebar = config_map(vec![("contents", arr(vec![s("a.qmd"), s("b.qmd")]))]);
        meta.insert_path(&["sidebar"], sidebar);

        let index = make_index_with(&["a.qmd", "b.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].members,
            vec![PathBuf::from("a.qmd"), PathBuf::from("b.qmd")]
        );
    }

    #[test]
    fn top_level_sidebar_concatenates_with_website_sidebar() {
        // Both locations present: the merged sidebar should pick up
        // entries from both layers. Arrays default to !concat, so the
        // resulting `contents` is the concatenation (nested first).
        let mut meta = ConfigValue::default();
        let nested = config_map(vec![("contents", arr(vec![s("a.qmd")]))]);
        let top = config_map(vec![("contents", arr(vec![s("b.qmd")]))]);
        meta.insert_path(&["website", "sidebar"], nested);
        meta.insert_path(&["sidebar"], top);

        let index = make_index_with(&["a.qmd", "b.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved.len(), 1);
        // Both members appear; concat order is nested-then-top.
        assert_eq!(
            resolved[0].members,
            vec![PathBuf::from("a.qmd"), PathBuf::from("b.qmd")]
        );
    }

    #[test]
    fn external_urls_excluded() {
        let mut meta = ConfigValue::default();
        let sidebar = config_map(vec![(
            "contents",
            arr(vec![s("a.qmd"), s("https://example.com")]),
        )]);
        meta.insert_path(&["website", "sidebar"], sidebar);

        let index = make_index_with(&["a.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        // External URL is dropped from membership; only project pages remain.
        assert_eq!(resolved[0].members, vec![PathBuf::from("a.qmd")]);
    }

    #[test]
    fn duplicate_paths_deduped() {
        let mut meta = ConfigValue::default();
        let section = config_map(vec![
            ("section", s("Group")),
            ("contents", arr(vec![s("a.qmd"), s("a.qmd")])),
        ]);
        let sidebar = config_map(vec![("contents", arr(vec![s("a.qmd"), section]))]);
        meta.insert_path(&["website", "sidebar"], sidebar);

        let index = make_index_with(&["a.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved[0].members, vec![PathBuf::from("a.qmd")]);
    }
}

/*
 * sidebar_auto.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! `auto:` expansion for sidebars.
//!
//! The Generate transform ([`SidebarGenerateTransform`]) calls
//! [`expand_auto`] to replace every
//! [`SidebarEntry::Auto`](quarto_navigation::SidebarEntry::Auto) in a
//! sidebar's contents with concrete link / section entries derived from
//! the project's set of documents.
//!
//! The resulting entries are **format-agnostic** — links carry their
//! source paths (`docs/api.qmd`), not output hrefs — so downstream
//! consumers (Render, cross-doc link rewrite) can map to their own
//! format's extension.
//!
//! See `claude-notes/plans/2026-04-24-websites-phase-2.md` §"Auto
//! expansion".
//!
//! [`SidebarGenerateTransform`]: crate::transforms::SidebarGenerateTransform

use quarto_error_reporting::DiagnosticMessage;
use quarto_navigation::{AutoSpec, NavigationItem, Sidebar, SidebarEntry};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::SourceInfo;

use crate::document_profile::DocumentProfile;
use crate::project::index::ProjectIndex;

/// Walk the sidebar's contents and expand every `Auto` entry in
/// place, using the project's `ProjectIndex`.
///
/// Diagnostics are pushed onto `diagnostics` for conditions like
/// "no profiles matched the `auto:` spec", but expansion itself is
/// best-effort: a miss produces an empty expansion, not an error.
pub fn expand_auto(
    sidebar: &mut Sidebar,
    index: &ProjectIndex,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    sidebar.contents = expand_entries(std::mem::take(&mut sidebar.contents), index, diagnostics);
}

/// Drop all `Auto` entries and emit a warning for each — used when
/// there is no `ProjectIndex` available (standalone render).
pub fn strip_auto(sidebar: &mut Sidebar, diagnostics: &mut Vec<DiagnosticMessage>) {
    sidebar.contents = strip_entries(std::mem::take(&mut sidebar.contents), diagnostics);
}

fn expand_entries(
    entries: Vec<SidebarEntry>,
    index: &ProjectIndex,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<SidebarEntry> {
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry {
            SidebarEntry::Auto(spec) => {
                out.extend(expand_spec(&spec, index, diagnostics));
            }
            SidebarEntry::Section {
                text,
                href,
                id,
                contents,
                expanded,
            } => {
                // Recurse into nested sections so `auto:` nested under a
                // hand-written section also expands.
                let new_contents = expand_entries(contents, index, diagnostics);
                out.push(SidebarEntry::Section {
                    text,
                    href,
                    id,
                    contents: new_contents,
                    expanded,
                });
            }
            other => out.push(other),
        }
    }
    out
}

fn strip_entries(
    entries: Vec<SidebarEntry>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<SidebarEntry> {
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry {
            SidebarEntry::Auto(_) => {
                diagnostics.push(DiagnosticMessage::warning(
                    "Sidebar `auto:` entry ignored — no project index is available. \
                     This usually means the document is being rendered standalone, \
                     not as part of a project."
                        .to_string(),
                ));
            }
            SidebarEntry::Section {
                text,
                href,
                id,
                contents,
                expanded,
            } => {
                let new_contents = strip_entries(contents, diagnostics);
                out.push(SidebarEntry::Section {
                    text,
                    href,
                    id,
                    contents: new_contents,
                    expanded,
                });
            }
            other => out.push(other),
        }
    }
    out
}

/// Expand a single `AutoSpec` into concrete sidebar entries.
pub fn expand_spec(
    spec: &AutoSpec,
    index: &ProjectIndex,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<SidebarEntry> {
    let (candidates, scope) = collect_candidates(spec, index);

    if candidates.is_empty() {
        diagnostics.push(DiagnosticMessage::warning(format!(
            "Sidebar `auto:` matched no documents (spec: {})",
            auto_spec_debug(spec)
        )));
        return Vec::new();
    }

    match scope {
        Scope::All => group_with_subdirs(&candidates, index),
        Scope::Flat => flatten_as_links(&candidates),
    }
}

fn auto_spec_debug(spec: &AutoSpec) -> String {
    match spec {
        AutoSpec::All => "auto: true".to_string(),
        AutoSpec::Path(p) => format!("auto: \"{}\"", p),
        AutoSpec::Paths(ps) => format!("auto: {:?}", ps),
    }
}

/// The "scope" of an auto spec — whether it's a whole-project sweep
/// (which may produce grouped Sections) or a scoped sub-path (which
/// produces a flat list).
enum Scope {
    All,
    Flat,
}

fn collect_candidates<'a>(
    spec: &AutoSpec,
    index: &'a ProjectIndex,
) -> (Vec<&'a DocumentProfile>, Scope) {
    let profiles = index.profiles();
    match spec {
        AutoSpec::All => {
            let candidates: Vec<&DocumentProfile> = profiles
                .iter()
                .filter(|p| !p.draft)
                .filter(|p| !is_top_level_index(p))
                .collect();
            (candidates, Scope::All)
        }
        AutoSpec::Path(pat) => {
            let normalized = normalize_pattern(pat);
            let candidates: Vec<&DocumentProfile> = profiles
                .iter()
                .filter(|p| !p.draft)
                .filter(|p| matches_prefix(p, &normalized))
                .collect();
            (candidates, Scope::Flat)
        }
        AutoSpec::Paths(pats) => {
            let normalized: Vec<String> = pats.iter().map(|p| normalize_pattern(p)).collect();
            let candidates: Vec<&DocumentProfile> = profiles
                .iter()
                .filter(|p| !p.draft)
                .filter(|p| normalized.iter().any(|n| matches_prefix(p, n)))
                .collect();
            // A Paths spec is always flat — grouping semantics are
            // ambiguous when multiple prefixes overlap.
            (candidates, Scope::Flat)
        }
    }
}

/// Strip trailing glob markers so `"docs"`, `"docs/"`, `"docs/*"`,
/// `"docs/**"`, `"docs/*.qmd"` all normalize to `"docs"`.
fn normalize_pattern(p: &str) -> String {
    let trimmed = p
        .trim_end_matches("*.qmd")
        .trim_end_matches("**")
        .trim_end_matches('*')
        .trim_end_matches('/');
    trimmed.to_string()
}

fn matches_prefix(profile: &DocumentProfile, prefix: &str) -> bool {
    let src = source_fwd_slash(profile);
    if prefix.is_empty() {
        return true;
    }
    // Exact match on a file path, or any descendant under a directory.
    src == prefix || src.starts_with(&format!("{}/", prefix))
}

fn source_fwd_slash(profile: &DocumentProfile) -> String {
    profile.source_path.to_string_lossy().replace('\\', "/")
}

/// Check whether a profile is the project's top-level `index.qmd` (or
/// similar `index.*` filename at the root). Q1 excludes this from the
/// sibling list of an `auto: true` expansion.
fn is_top_level_index(profile: &DocumentProfile) -> bool {
    let src = source_fwd_slash(profile);
    if src.contains('/') {
        return false;
    }
    if let Some(stem) = profile.source_path.file_stem().and_then(|s| s.to_str()) {
        return stem.eq_ignore_ascii_case("index");
    }
    false
}

/// Group a candidate list into top-level links + per-subdir sections.
/// Used for `auto: true`.
fn group_with_subdirs(candidates: &[&DocumentProfile], index: &ProjectIndex) -> Vec<SidebarEntry> {
    // Partition: top-level files vs items grouped by their first path
    // component. Built as a `Vec<(dir_name, Vec<&Profile>)>` so
    // insertion order is preserved deterministically (independent of
    // HashMap iteration order) from `candidates`'s order, which is
    // itself the `ProjectIndex`'s insertion order.
    let mut top_level: Vec<&DocumentProfile> = Vec::new();
    let mut dir_groups: Vec<(String, Vec<&DocumentProfile>)> = Vec::new();

    for profile in candidates {
        let src = source_fwd_slash(profile);
        match src.split_once('/') {
            None => top_level.push(profile),
            Some((dir_ref, _)) => {
                let dir = dir_ref.to_string();
                if let Some(group) = dir_groups.iter_mut().find(|g| g.0 == dir) {
                    group.1.push(profile);
                } else {
                    dir_groups.push((dir, vec![profile]));
                }
            }
        }
    }

    let mut out = Vec::<SidebarEntry>::new();
    // Top-level links first, sorted.
    let mut top_level = top_level;
    sort_profiles(&mut top_level);
    for p in top_level {
        out.push(link_entry(p));
    }

    // Then subdirectory sections.
    for (dir, mut members) in dir_groups {
        sort_profiles(&mut members);
        out.push(section_for_dir(&dir, &members, index));
    }

    out
}

fn section_for_dir(dir: &str, members: &[&DocumentProfile], index: &ProjectIndex) -> SidebarEntry {
    // Find the directory's own `index.*` if present. Only `.qmd` is
    // discoverable by Phase-1 project walking; that's fine for MVP.
    let index_src = format!("{}/index.qmd", dir);
    let index_profile = index.lookup_by_source(std::path::Path::new(&index_src));

    let (text_cv, href) = match index_profile {
        Some(p) => {
            let title = p.title.clone().unwrap_or_else(|| capitalize(dir));
            (
                Some(ConfigValue::new_string(&title, SourceInfo::default())),
                Some(index_src.clone()),
            )
        }
        None => (
            Some(ConfigValue::new_string(
                &capitalize(dir),
                SourceInfo::default(),
            )),
            None,
        ),
    };

    // Exclude the index from the child list (it's already the section
    // header's href).
    let contents: Vec<SidebarEntry> = members
        .iter()
        .filter(|p| source_fwd_slash(p) != index_src)
        .map(|p| link_entry(p))
        .collect();

    SidebarEntry::Section {
        text: text_cv,
        href,
        id: None,
        contents,
        expanded: false,
    }
}

fn flatten_as_links(candidates: &[&DocumentProfile]) -> Vec<SidebarEntry> {
    let mut sorted: Vec<&DocumentProfile> = candidates.to_vec();
    sort_profiles(&mut sorted);
    sorted.into_iter().map(link_entry).collect()
}

fn link_entry(profile: &DocumentProfile) -> SidebarEntry {
    let href = source_fwd_slash(profile);
    let text = profile.title.clone().unwrap_or_else(|| {
        // Fall back to the file stem.
        profile
            .source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(capitalize)
            .unwrap_or_else(|| href.clone())
    });
    SidebarEntry::Link {
        item: NavigationItem {
            href: Some(href),
            text: Some(ConfigValue::new_string(&text, SourceInfo::default())),
            ..NavigationItem::default()
        },
    }
}

/// Sort candidates by `order:` (asc, None last) then title (case-
/// insensitive alphabetical), matching Q1.
fn sort_profiles(list: &mut [&DocumentProfile]) {
    list.sort_by(|a, b| {
        // None sorts AFTER Some — so both orders are matched on value,
        // None is treated as +inf.
        let order_cmp = match (a.order, b.order) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        if order_cmp != std::cmp::Ordering::Equal {
            return order_cmp;
        }
        // Fall back to title (case-insensitive).
        let ta = a
            .title
            .as_deref()
            .map(str::to_lowercase)
            .unwrap_or_else(|| source_fwd_slash(a).to_lowercase());
        let tb = b
            .title
            .as_deref()
            .map(str::to_lowercase)
            .unwrap_or_else(|| source_fwd_slash(b).to_lowercase());
        ta.cmp(&tb)
    });
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_profile(source: &str, title: &str) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: source.replace(".qmd", ".html"),
            format_id: "html".to_string(),
            title: Some(title.to_string()),
            ..DocumentProfile::default()
        }
    }

    fn make_profile_draft(source: &str, title: &str) -> DocumentProfile {
        let mut p = make_profile(source, title);
        p.draft = true;
        p
    }

    fn make_profile_order(source: &str, title: &str, order: i32) -> DocumentProfile {
        let mut p = make_profile(source, title);
        p.order = Some(order);
        p
    }

    /// Test 19 — `auto: true` enumerates all non-draft, non-top-index
    /// profiles.
    #[test]
    fn auto_true_lists_all_renderable_profiles() {
        let profiles = vec![
            make_profile("a.qmd", "A"),
            make_profile("b.qmd", "B"),
            make_profile("c.qmd", "C"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::All, &index, &mut diags);
        assert_eq!(entries.len(), 3);
        assert!(diags.is_empty());
        for entry in &entries {
            assert!(matches!(entry, SidebarEntry::Link { .. }));
        }
    }

    /// Test 20 — top-level `index.qmd` is excluded from sibling list.
    #[test]
    fn auto_excludes_index_as_sibling() {
        let profiles = vec![
            make_profile("index.qmd", "Home"),
            make_profile("a.qmd", "A"),
            make_profile("b.qmd", "B"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::All, &index, &mut diags);
        assert_eq!(entries.len(), 2);
        for entry in &entries {
            match entry {
                SidebarEntry::Link { item, .. } => {
                    let href = item.href.as_deref().unwrap();
                    assert!(
                        !href.contains("index.qmd"),
                        "index.qmd leaked into siblings"
                    );
                }
                other => panic!("expected Link, got {:?}", other),
            }
        }
    }

    /// Test 21 — `auto: docs` scopes to the subdirectory and flattens.
    #[test]
    fn auto_path_scopes_to_subdir() {
        let profiles = vec![
            make_profile("a.qmd", "A"),
            make_profile("docs/b.qmd", "B"),
            make_profile("docs/c.qmd", "C"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::Path("docs".to_string()), &index, &mut diags);
        assert_eq!(entries.len(), 2);
        for entry in &entries {
            match entry {
                SidebarEntry::Link { item, .. } => {
                    let href = item.href.as_deref().unwrap();
                    assert!(href.starts_with("docs/"), "got href {}", href);
                }
                other => panic!("expected Link, got {:?}", other),
            }
        }
    }

    /// `auto: docs/*` normalizes to the same as `auto: docs`.
    #[test]
    fn auto_path_with_glob_normalizes() {
        let profiles = vec![make_profile("a.qmd", "A"), make_profile("docs/b.qmd", "B")];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::Path("docs/*".to_string()), &index, &mut diags);
        assert_eq!(entries.len(), 1);
    }

    /// Test 22 — `auto: true` with a subdir index.qmd produces a
    /// Section whose href is `docs/index.qmd` (source path), not
    /// `docs/index.html`.
    #[test]
    fn auto_groups_into_section_with_index() {
        let profiles = vec![
            make_profile("docs/index.qmd", "Docs Home"),
            make_profile("docs/b.qmd", "B"),
            make_profile("docs/c.qmd", "C"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::All, &index, &mut diags);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            SidebarEntry::Section {
                href,
                text,
                contents,
                ..
            } => {
                assert_eq!(
                    href.as_deref(),
                    Some("docs/index.qmd"),
                    "Section.href must stay a source path (Generate is format-agnostic)"
                );
                assert_eq!(
                    text.as_ref().unwrap().as_plain_text().as_deref(),
                    Some("Docs Home")
                );
                assert_eq!(contents.len(), 2, "two children (b, c) — index excluded");
            }
            other => panic!("expected Section, got {:?}", other),
        }
    }

    /// Test 23 — sort by `order:` ascending, then title.
    #[test]
    fn auto_sorts_by_order_then_title() {
        let profiles = vec![
            make_profile("c.qmd", "Charlie"),
            make_profile_order("a.qmd", "Alpha", 2),
            make_profile_order("b.qmd", "Bravo", 1),
            make_profile("d.qmd", "Delta"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::All, &index, &mut diags);
        assert_eq!(entries.len(), 4);
        let titles: Vec<String> = entries
            .iter()
            .map(|e| match e {
                SidebarEntry::Link { item, .. } => {
                    item.text.as_ref().and_then(|v| v.as_plain_text()).unwrap()
                }
                _ => panic!("expected Link"),
            })
            .collect();
        // Expected order: Bravo (order=1), Alpha (order=2), then
        // Charlie, Delta alphabetically (no order).
        assert_eq!(titles, vec!["Bravo", "Alpha", "Charlie", "Delta"]);
    }

    /// Test 24 — drafts are excluded.
    #[test]
    fn auto_drops_drafts() {
        let profiles = vec![
            make_profile("a.qmd", "A"),
            make_profile_draft("b.qmd", "B"),
            make_profile("c.qmd", "C"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::All, &index, &mut diags);
        assert_eq!(entries.len(), 2);
        for entry in &entries {
            match entry {
                SidebarEntry::Link { item, .. } => {
                    assert_ne!(item.href.as_deref(), Some("b.qmd"));
                }
                _ => panic!("expected Link"),
            }
        }
    }

    /// Test 25 — when no index is available, `strip_auto` drops the
    /// Auto entry and emits a warning.
    #[test]
    fn auto_without_index_is_noop() {
        let mut sb = Sidebar {
            contents: vec![
                SidebarEntry::Link {
                    item: NavigationItem {
                        href: Some("a.qmd".to_string()),
                        ..NavigationItem::default()
                    },
                },
                SidebarEntry::Auto(AutoSpec::All),
            ],
            ..Sidebar::with_defaults()
        };
        let mut diags = Vec::new();
        strip_auto(&mut sb, &mut diags);
        assert_eq!(sb.contents.len(), 1); // Auto dropped
        assert_eq!(diags.len(), 1); // warning emitted
    }

    /// `AutoSpec::Paths(vec![...])` is the union of multiple prefixes,
    /// always flattened.
    #[test]
    fn auto_paths_is_union_flat() {
        let profiles = vec![
            make_profile("a.qmd", "A"),
            make_profile("docs/b.qmd", "B"),
            make_profile("other.qmd", "Other"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(
            &AutoSpec::Paths(vec!["docs".to_string(), "other.qmd".to_string()]),
            &index,
            &mut diags,
        );
        assert_eq!(entries.len(), 2);
        // All Links, no Section (Paths is always flat).
        for entry in &entries {
            assert!(matches!(entry, SidebarEntry::Link { .. }));
        }
    }

    /// `expand_auto` traverses into nested sections.
    #[test]
    fn expand_auto_recurses_into_sections() {
        let profiles = vec![make_profile("a.qmd", "A"), make_profile("b.qmd", "B")];
        let index = ProjectIndex::new(profiles);
        let inner_auto = SidebarEntry::Auto(AutoSpec::All);
        let mut sb = Sidebar {
            contents: vec![SidebarEntry::Section {
                text: Some(ConfigValue::new_string("Outer", SourceInfo::default())),
                href: None,
                id: None,
                contents: vec![inner_auto],
                expanded: false,
            }],
            ..Sidebar::with_defaults()
        };
        let mut diags = Vec::new();
        expand_auto(&mut sb, &index, &mut diags);
        match &sb.contents[0] {
            SidebarEntry::Section { contents, .. } => {
                assert_eq!(
                    contents.len(),
                    2,
                    "auto inside section should have expanded"
                );
                for entry in contents {
                    assert!(matches!(entry, SidebarEntry::Link { .. }));
                }
            }
            _ => panic!("outer Section lost"),
        }
    }
}

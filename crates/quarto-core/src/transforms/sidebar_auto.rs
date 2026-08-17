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

use std::path::Path;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_navigation::{AutoSpec, NavigationItem, Sidebar, SidebarEntry};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::{By, SourceInfo};

use crate::document_profile::DocumentProfile;
use crate::glob::{BaseDirContext, GlobOptions, PatternSet, RawGlob, resolve_patterns};
use crate::project::index::ProjectIndex;

/// Q-13-5: sidebar `auto:` dropped because no project index is
/// available (standalone render).
fn auto_no_index_warning() -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Sidebar `auto:` ignored")
        .with_code("Q-13-5")
        .problem("No project index is available — `auto:` entries cannot be expanded.")
        .add_hint("Render this document as part of a project to expand `auto:` entries.")
        .build()
}

/// Q-13-6: sidebar `auto:` expanded against the project index but
/// matched no documents.
fn auto_empty_match_warning(spec: &AutoSpec) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Sidebar `auto:` matched no documents")
        .with_code("Q-13-6")
        .problem(format!(
            "Spec `{}` found no matches.",
            auto_spec_debug(spec)
        ))
        .add_hint(
            "Check the path or glob pattern, or confirm the target files exist in the project.",
        )
        .build()
}

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
                href_source,
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
                    href_source,
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
                diagnostics.push(auto_no_index_warning());
            }
            SidebarEntry::Section {
                text,
                href,
                href_source,
                id,
                contents,
                expanded,
            } => {
                let new_contents = strip_entries(contents, diagnostics);
                out.push(SidebarEntry::Section {
                    text,
                    href,
                    href_source,
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
        diagnostics.push(auto_empty_match_warning(spec));
        return Vec::new();
    }

    match scope {
        Scope::Grouped => group_with_subdirs(&candidates, index),
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

/// The "scope" of an auto spec — whether its matches get partitioned
/// into per-directory Sections, or emitted as a flat list of links.
enum Scope {
    Grouped,
    Flat,
}

/// Does this `auto:` pattern name a **directory**?
///
/// bd-sidebar-contents-dir-shorthand-z7arvhx8 D3. Q1 answers this by
/// stat-ing the filesystem (`safeExistsSync` + `isDirectory` in
/// `website-sidebar-auto.ts`). q2 cannot: expansion is driven by the
/// `ProjectIndex` and has to work under WASM, where there is no
/// filesystem to stat. So we answer it from the index instead — a
/// pattern is a directory when it carries no glob metacharacter *and*
/// at least one matched document lives beneath it.
///
/// The deliberate consequence (accepted when this was designed): an
/// empty or unindexed directory is not recognised as one. It matches
/// nothing, so `expand_spec` returns early with the `Q-13-6`
/// empty-match warning before scope is ever consulted.
fn is_bare_directory(pattern: &str, candidates: &[&DocumentProfile]) -> bool {
    let pattern = pattern.trim_end_matches('/');
    if pattern.is_empty() || pattern.contains(['*', '?', '[', ']', '{', '}']) {
        return false;
    }
    let prefix = format!("{}/", pattern);
    candidates
        .iter()
        .any(|p| source_fwd_slash(p).starts_with(&prefix))
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
            (candidates, Scope::Grouped)
        }
        AutoSpec::Path(pat) => {
            let matcher = auto_matcher(std::slice::from_ref(pat));
            let candidates: Vec<&DocumentProfile> = profiles
                .iter()
                .filter(|p| !p.draft)
                .filter(|p| matches_spec(p, matcher.as_ref()))
                .collect();
            // D1 — a bare directory is Q1's "auto-dir": the directory
            // becomes a wrapping node. Every candidate then shares a
            // first path component, so `group_with_subdirs` collapses
            // to exactly one titled section. Globs stay flat.
            let scope = if is_bare_directory(pat, &candidates) {
                Scope::Grouped
            } else {
                Scope::Flat
            };
            (candidates, scope)
        }
        AutoSpec::Paths(pats) => {
            let matcher = auto_matcher(pats);
            let candidates: Vec<&DocumentProfile> = profiles
                .iter()
                .filter(|p| !p.draft)
                .filter(|p| matches_spec(p, matcher.as_ref()))
                .collect();
            // A Paths spec is always flat — grouping semantics are
            // ambiguous when multiple patterns overlap.
            (candidates, Scope::Flat)
        }
    }
}

/// Compile an `auto:` spec's patterns with q2's shared glob
/// semantics (bd-mt7a6uc4 D6).
///
/// Before this, `auto:` was not a glob implementation at all: it
/// stripped `*.qmd` / `**` / `*` off the end of each entry and
/// prefix-matched what was left, so `docs/*.qmd`, `docs/**` and
/// `docs` were the same pattern and every one of them swept up
/// nested documents. Now `*` is one directory level, `**` crosses
/// levels, a bare directory still means everything beneath it, and
/// `[a-z]` classes and `!` exclusions work — the same rules
/// `contents:`, `project.render` and `resources:` follow.
///
/// Patterns resolve against the **project root**. `auto:` lists
/// project pages, and `AutoSpec` carries no provenance, so there is
/// no declaring-file directory to anchor to.
///
/// Returns `None` when nothing compiles, which matches nothing —
/// the caller's existing `Q-13-6` empty-match warning covers it.
fn auto_matcher(patterns: &[String]) -> Option<PatternSet> {
    let resolution = resolve_patterns(
        patterns
            .iter()
            .map(|p| RawGlob::new(p.clone(), SourceInfo::generated(By::programmatic_config()))),
        &BaseDirContext {
            source_context: None,
            project_dir: Path::new(""),
            fallback_dir: "",
        },
        &GlobOptions::SIDEBAR,
    );
    resolution.compile(&GlobOptions::SIDEBAR).ok()
}

fn matches_spec(profile: &DocumentProfile, matcher: Option<&PatternSet>) -> bool {
    matcher.is_some_and(|m| m.matches(&source_fwd_slash(profile)))
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
                Some(ConfigValue::new_string(
                    &title,
                    SourceInfo::generated(By::programmatic_config()),
                )),
                Some(index_src.clone()),
            )
        }
        None => (
            Some(ConfigValue::new_string(
                capitalize(dir),
                SourceInfo::generated(By::programmatic_config()),
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
        // `auto:` expansion produces synthetic entries — no source
        // YAML to point back at. Default SourceInfo is the safe
        // sentinel for "programmatically constructed."
        href_source: SourceInfo::generated(By::programmatic_config()),
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
            .map_or_else(|| href.clone(), capitalize)
    });
    SidebarEntry::Link {
        item: NavigationItem {
            href: Some(href),
            text: Some(ConfigValue::new_string(
                &text,
                SourceInfo::generated(By::programmatic_config()),
            )),
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
            .map_or_else(|| source_fwd_slash(a).to_lowercase(), str::to_lowercase);
        let tb = b
            .title
            .as_deref()
            .map_or_else(|| source_fwd_slash(b).to_lowercase(), str::to_lowercase);
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

    /// Test 21 — `auto: docs` scopes to the subdirectory and wraps the
    /// matches in a section titled after the directory.
    ///
    /// bd-sidebar-contents-dir-shorthand-z7arvhx8 D1: this asserted a
    /// *flat* list of links until Q1 parity landed. Q1 treats a bare
    /// directory as an auto-dir and re-roots the node set so the
    /// directory becomes a wrapping node (`isAutoDir` in
    /// `website-sidebar-auto.ts`), which is what the section below is.
    /// Globs (`docs/*`, `docs/**/*.qmd`) are *not* directories and stay
    /// flat — see the tests that follow.
    #[test]
    fn auto_bare_directory_wraps_in_titled_section() {
        let profiles = vec![
            make_profile("a.qmd", "A"),
            make_profile("docs/b.qmd", "B"),
            make_profile("docs/c.qmd", "C"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::Path("docs".to_string()), &index, &mut diags);
        assert_eq!(entries.len(), 1, "one section, not a flat run of links");
        match &entries[0] {
            SidebarEntry::Section {
                text,
                href,
                contents,
                ..
            } => {
                assert_eq!(
                    text.as_ref().unwrap().as_plain_text().as_deref(),
                    Some("Docs"),
                    "no docs/index.qmd, so the title is the capitalized dir name"
                );
                assert_eq!(href.as_deref(), None, "no index.qmd means no header link");
                assert_eq!(contents.len(), 2, "b and c");
                for entry in contents {
                    match entry {
                        SidebarEntry::Link { item, .. } => {
                            let href = item.href.as_deref().unwrap();
                            assert!(href.starts_with("docs/"), "got href {}", href);
                        }
                        other => panic!("expected Link, got {:?}", other),
                    }
                }
            }
            other => panic!("expected Section, got {:?}", other),
        }
    }

    /// D1 + D3 — a bare directory that *has* an `index.qmd` takes its
    /// title and href from that index, and excludes it from the
    /// children. This is the shape Q1 emits for the Connect docs'
    /// `contents: how-to`.
    #[test]
    fn auto_bare_directory_section_uses_dir_index() {
        let profiles = vec![
            make_profile("how-to/index.qmd", "How To Guides"),
            make_profile("how-to/one.qmd", "One"),
            make_profile("how-to/two.qmd", "Two"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::Path("how-to".to_string()), &index, &mut diags);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            SidebarEntry::Section {
                text,
                href,
                contents,
                ..
            } => {
                assert_eq!(
                    text.as_ref().unwrap().as_plain_text().as_deref(),
                    Some("How To Guides")
                );
                assert_eq!(href.as_deref(), Some("how-to/index.qmd"));
                assert_eq!(contents.len(), 2, "index is the header, not a child");
            }
            other => panic!("expected Section, got {:?}", other),
        }
    }

    /// D3 — directory-ness is decided from the *project index*, never
    /// the filesystem (WASM-safe). A spec naming a single document is
    /// not a directory, so it stays a flat link rather than becoming a
    /// section wrapping itself.
    #[test]
    fn auto_single_file_spec_is_not_a_directory() {
        let profiles = vec![
            make_profile("intro.qmd", "Intro"),
            make_profile("a.qmd", "A"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::Path("intro.qmd".to_string()), &index, &mut diags);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            SidebarEntry::Link { item, .. } => {
                assert_eq!(item.href.as_deref(), Some("intro.qmd"));
            }
            other => panic!("expected Link, got {:?}", other),
        }
    }

    /// D1 — an explicit glob is not a bare directory even when it
    /// happens to select a whole directory's worth of documents. This
    /// is the fence that keeps `docs/*` from silently becoming a
    /// section.
    #[test]
    fn auto_glob_spec_stays_flat() {
        let profiles = vec![
            make_profile("docs/b.qmd", "B"),
            make_profile("docs/c.qmd", "C"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::Path("docs/*".to_string()), &index, &mut diags);
        assert_eq!(entries.len(), 2, "glob stays flat");
        assert!(
            entries
                .iter()
                .all(|e| matches!(e, SidebarEntry::Link { .. })),
            "expected all Links, got {:?}",
            entries
        );
    }

    /// `auto: docs/*` matches the documents directly in `docs/` —
    /// one directory level, like `*` everywhere else in q2.
    #[test]
    fn auto_path_with_glob_matches_one_level() {
        let profiles = vec![make_profile("a.qmd", "A"), make_profile("docs/b.qmd", "B")];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::Path("docs/*".to_string()), &index, &mut diags);
        assert_eq!(entries.len(), 1);
    }

    /// bd-mt7a6uc4 D6 — the behavior change. `auto: "docs/*.qmd"`
    /// used to be normalized to the prefix `docs` and therefore
    /// swept up nested documents; now `*` means one directory level
    /// and the author writes `**/` to recurse.
    #[test]
    fn auto_single_star_no_longer_matches_nested_documents() {
        let profiles = vec![
            make_profile("docs/top.qmd", "Top"),
            make_profile("docs/deep/nested.qmd", "Nested"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(
            &AutoSpec::Path("docs/*.qmd".to_string()),
            &index,
            &mut diags,
        );
        assert_eq!(entries.len(), 1, "only docs/top.qmd");
    }

    #[test]
    fn auto_double_star_matches_nested_documents() {
        let profiles = vec![
            make_profile("docs/top.qmd", "Top"),
            make_profile("docs/deep/nested.qmd", "Nested"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(
            &AutoSpec::Path("docs/**/*.qmd".to_string()),
            &index,
            &mut diags,
        );
        assert_eq!(entries.len(), 2);
    }

    /// A bare directory still *matches* everything beneath it
    /// (bd-mt7a6uc4 D4), including nested documents. Since
    /// bd-sidebar-contents-dir-shorthand-z7arvhx8 D1 those matches
    /// arrive wrapped in the directory's section rather than as a flat
    /// run, so the count is asserted on the section's children.
    ///
    /// Note the known limitation recorded in that plan: `section_for_dir`
    /// builds children as flat links, so `docs/deep/nested.qmd` appears
    /// as a plain entry rather than a nested `deep` sub-section. Q1
    /// recurses arbitrarily; q2 is one level deep here.
    #[test]
    fn auto_bare_directory_still_matches_nested_documents() {
        let profiles = vec![
            make_profile("docs/top.qmd", "Top"),
            make_profile("docs/deep/nested.qmd", "Nested"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();
        let entries = expand_spec(&AutoSpec::Path("docs".to_string()), &index, &mut diags);
        assert_eq!(entries.len(), 1, "one section for docs/");
        match &entries[0] {
            SidebarEntry::Section { contents, .. } => {
                assert_eq!(contents.len(), 2, "top and the nested document");
                let hrefs: Vec<&str> = contents
                    .iter()
                    .map(|e| match e {
                        SidebarEntry::Link { item, .. } => item.href.as_deref().unwrap(),
                        other => panic!("expected Link, got {:?}", other),
                    })
                    .collect();
                assert!(hrefs.contains(&"docs/deep/nested.qmd"));
            }
            other => panic!("expected Section, got {:?}", other),
        }
    }

    /// Character classes reach `auto:` too, and `!` excludes.
    #[test]
    fn auto_supports_classes_and_negation() {
        let profiles = vec![
            make_profile("docs/ch-1.qmd", "One"),
            make_profile("docs/ch-2.qmd", "Two"),
            make_profile("docs/notes.qmd", "Notes"),
        ];
        let index = ProjectIndex::new(profiles);
        let mut diags = Vec::new();

        let entries = expand_spec(
            &AutoSpec::Path("docs/ch-[0-9].qmd".to_string()),
            &index,
            &mut diags,
        );
        assert_eq!(entries.len(), 2);

        let entries = expand_spec(
            &AutoSpec::Paths(vec!["docs".to_string(), "!docs/notes.qmd".to_string()]),
            &index,
            &mut diags,
        );
        assert_eq!(entries.len(), 2, "negation excludes docs/notes.qmd");
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
    /// Auto entry and emits a structured Q-13-5 warning.
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
        let d = &diags[0];
        assert_eq!(d.code.as_deref(), Some("Q-13-5"));
        assert!(
            d.title.contains("`auto:`"),
            "Q-13-5 title must mention `auto:`; got {:?}",
            d.title
        );
        assert!(
            d.problem
                .as_ref()
                .is_some_and(|p| p.as_str().contains("project index")),
            "Q-13-5 problem must mention project index; got {:?}",
            d.problem
        );
    }

    /// bd-8d6rk: `expand_spec` with no matches emits a structured
    /// Q-13-6 warning and produces no entries.
    #[test]
    fn auto_empty_match_emits_q_13_6() {
        let index = ProjectIndex::new(vec![]);
        let mut diags = Vec::new();
        let entries = expand_spec(
            &AutoSpec::Path("nonexistent".to_string()),
            &index,
            &mut diags,
        );
        assert!(entries.is_empty());
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.code.as_deref(), Some("Q-13-6"));
        assert!(
            d.title.contains("no documents"),
            "Q-13-6 title must mention `no documents`; got {:?}",
            d.title
        );
        assert!(
            d.problem
                .as_ref()
                .is_some_and(|p| p.as_str().contains("nonexistent")),
            "Q-13-6 problem must include the spec text; got {:?}",
            d.problem
        );
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
                text: Some(ConfigValue::new_string("Outer", SourceInfo::for_test())),
                href: None,
                href_source: SourceInfo::for_test(),
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

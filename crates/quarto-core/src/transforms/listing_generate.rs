/*
 * listing_generate.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Pass-2 listing-resolution transform.
//!
//! Reads the host page's `listing:` frontmatter, resolves each
//! declared listing into a [`ResolvedListing`] (config plus the
//! hydrated, filtered, sorted, truncated item set), and stores the
//! results on
//! [`RenderContext::resolved_listings`](crate::render::RenderContext)
//! for [`ListingRenderTransform`](super::listing_render::
//! ListingRenderTransform) to consume.
//!
//! Item discovery (D10): no filesystem walk. Each listing's
//! `contents:` glob is resolved to a project-relative pattern
//! against the directory of the file it was written in
//! ([`crate::project::listing::glob_resolve`]; GH #456,
//! bd-v7ixzsp5) and matched single-view against
//! [`crate::project::index::ProjectIndex::profiles`]. This gives
//! identical behavior on native and WASM, naturally excludes files
//! outside the project's render set, and naturally excludes the
//! host page itself. Patterns whose normalization escapes the
//! project root match nothing and emit `Q-12-17` here (this
//! transform owns the diagnostic; the profile stage drops them
//! silently).
//!
//! TODO(bd-0fd0): when a Lua filter slot lands between generate and
//! render transforms, serialize the resolved listings into
//! `meta.listings.<id>` at that boundary so user filters can
//! mutate them. Until then, the typed in-memory shape on
//! `RenderContext` is the only data path between this transform
//! and `ListingRenderTransform`.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::document_profile::DocumentProfile;
use crate::glob::{
    BaseDirContext, GlobOptions, PatternSet, has_metacharacters, join_and_normalize,
    path_to_forward_slashes,
};
use crate::project::index::ProjectIndex;
use crate::project::listing::config::is_markdown_document_path;
use crate::project::listing::filter::apply_filters;
use crate::project::listing::glob_resolve::resolve_content_globs;
use crate::project::listing::helpers::is_remote_src;
use crate::project::listing::record::{overlay_record, parse_record, record_item};
use crate::project::listing::sort::apply_sort;
use crate::project::listing::{
    ItemTarget, ListingContents, ListingItem, ResolvedListing, hydrate_item, parse_listings,
};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::is_feature_disabled;
use crate::transforms::navigation_active::page_relative_source;

pub struct ListingGenerateTransform;

impl ListingGenerateTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListingGenerateTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ListingGenerateTransform {
    fn name(&self) -> &str {
        "listing-generate"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "listing") {
            return Ok(());
        }

        let Some(listing_value) = ast.meta.get("listing") else {
            return Ok(());
        };

        // Skip-condition: another generate path (today: hand-edited
        // `RenderContext::resolved_listings`, tomorrow: a Lua filter
        // post-bd-0fd0) already populated the resolved set. Treat as
        // an override and bail out.
        if !ctx.resolved_listings.is_empty() {
            return Ok(());
        }

        let mut diags = std::mem::take(&mut ctx.diagnostics);
        let listings = parse_listings(listing_value, &mut diags);

        if listings.is_empty() {
            ctx.diagnostics = diags;
            return Ok(());
        }

        // Determine the host page's project-relative directory once.
        // Used to compute "host-directory-relative" item paths for
        // glob matching. The host page itself is dropped from item
        // discovery (Q1 default).
        let host_path_str = page_relative_source(ctx);
        let host_dir_str = std::path::Path::new(&host_path_str)
            .parent()
            .map(path_to_forward_slashes)
            .unwrap_or_default();

        let mut resolved: Vec<ResolvedListing> = Vec::with_capacity(listings.len());

        let base_ctx = BaseDirContext {
            source_context: ctx.source_context,
            project_dir: &ctx.project.dir,
            fallback_dir: &host_dir_str,
        };

        for listing in listings {
            // Q-12-23: a literal YAML-file entry is Q1's third item
            // source (bd-hj1ehfn8), not a glob that matched nothing.
            // Partition it out so it neither resolves nor trips Q-12-19.
            let mut contents: Vec<ListingContents> = Vec::with_capacity(listing.contents.len());
            for entry in &listing.contents {
                match entry {
                    ListingContents::Glob { pattern, source }
                        if !has_metacharacters(pattern)
                            && matches!(
                                std::path::Path::new(pattern)
                                    .extension()
                                    .and_then(|e| e.to_str()),
                                Some("yml" | "yaml")
                            ) =>
                    {
                        diags.push(yaml_contents_unsupported(pattern, source));
                    }
                    other => contents.push(other.clone()),
                }
            }

            // Resolve each glob against the directory of the file
            // it was written in (provenance-based; GH #456).
            let resolution = resolve_content_globs(
                &contents,
                ctx.source_context,
                &ctx.project.dir,
                &host_dir_str,
            );
            for escaped in &resolution.escaped {
                diags.push(
                    DiagnosticMessageBuilder::warning(format!(
                        "Listing `contents:` pattern `{}` points outside the project \
                         directory and matches nothing.",
                        escaped.raw
                    ))
                    .with_code("Q-12-17")
                    .with_location(escaped.source.clone())
                    .problem("The pattern's `..` segments climb above the project root.")
                    .add_info(
                        "Listing contents are limited to files inside the project. \
                         Adjust the pattern so it stays within the project directory.",
                    )
                    .build(),
                );
            }

            // Compile the full set once for the global negation
            // check, and each positive pattern individually: item
            // collection orders by the FIRST pattern that matches a
            // candidate (Q1's glob-major semantics,
            // bd-listing-declared-order-3ixcvc4o), and the Q-12-19
            // matched-nothing diagnostic credits EVERY pattern a
            // candidate matches. Resolution already validated these
            // patterns, so the compiles cannot fail; an empty set on
            // the impossible path simply matches nothing.
            let empty_set = || PatternSet::compile(&[], &GlobOptions::LISTING).unwrap();
            let patterns = resolution
                .compile(&GlobOptions::LISTING)
                .unwrap_or_else(|_| empty_set());
            let positives: Vec<_> = resolution.positives().collect();
            let positive_sets: Vec<PatternSet> = positives
                .iter()
                .map(|(glob, _)| {
                    PatternSet::compile(std::slice::from_ref(*glob), &GlobOptions::LISTING)
                        .unwrap_or_else(|_| empty_set())
                })
                .collect();
            let mut matched_any = vec![false; positive_sets.len()];

            // Patterns the glob engine rejected (bd-mt7a6uc4).
            // Before, these matched nothing in silence — the same
            // failure mode #456 fixed for asterisk-corrupted globs.
            for invalid in &resolution.invalid {
                diags.push(
                    crate::glob::diagnostics::invalid_pattern(
                        "Q-12-18",
                        "Listing `contents:`",
                        &invalid.raw,
                        &invalid.message,
                        &invalid.source,
                    )
                    .build(),
                );
            }

            // Candidate-major collection, pattern-major order: tag
            // each item with the index of its first matching
            // pattern, then stable-sort by that index. Exclusions
            // stay global — a `!` entry excludes a candidate no
            // matter where it appears in `contents:`.
            let mut ordered: Vec<((usize, u8), ListingItem)> = Vec::new();
            if let Some(index) = ctx.project_index.as_deref() {
                for profile in index.profiles() {
                    let candidate_path_str = path_to_forward_slashes(&profile.source_path);
                    if candidate_path_str == host_path_str {
                        continue;
                    }
                    if patterns.excluded(&candidate_path_str) {
                        continue;
                    }
                    if !item_visible(profile) {
                        continue;
                    }
                    let mut first_match: Option<usize> = None;
                    for (i, set) in positive_sets.iter().enumerate() {
                        if set.matches(&candidate_path_str) {
                            matched_any[i] = true;
                            first_match.get_or_insert(i);
                        }
                    }
                    if let Some(pattern_idx) = first_match {
                        ordered.push(((pattern_idx, 1), hydrate_item(profile)));
                    }
                }
            }

            // Second item source: inline records
            // (bd-listing-inline-contents-tyy446ze, plan §D2–D4).
            // `globs_before` is the record's declared position, on
            // the SAME scale as a glob item's `pattern_idx` above:
            // the ordinal a positive pattern declared at this point
            // would get in `resolution.positives()`. A negated
            // pattern, or one dropped as escaped (Q-12-17) or invalid
            // (Q-12-18), contributes no `pattern_idx` and so must not
            // advance this counter either — otherwise the two indices
            // drift apart and a record can sort after items it was
            // written before (review fix, task 5 round 1).
            //
            // `resolution.entry_index` gives this structurally — the
            // outcome resolution itself recorded for each raw entry,
            // aligned 1:1 with `contents`'s `Glob` entries in
            // declared order — rather than reconstructed afterward by
            // matching raw pattern text or `SourceInfo` (both proved
            // unreliable: the same text can escape from one declaring
            // file and resolve fine from another, and `SourceInfo`
            // can be a shared constant in programmatic/test
            // construction — review fix, task 5 round 2).
            // `positive_ordinal_by_glob_idx[j]` is the ordinal
            // `resolution.globs[j]` would get in `positives()`, or
            // `None` if it's negated.
            let mut positive_ordinal_by_glob_idx: Vec<Option<usize>> =
                Vec::with_capacity(resolution.globs.len());
            {
                let mut next_positive = 0usize;
                for g in &resolution.globs {
                    if g.negated {
                        positive_ordinal_by_glob_idx.push(None);
                    } else {
                        positive_ordinal_by_glob_idx.push(Some(next_positive));
                        next_positive += 1;
                    }
                }
            }
            // Starts at 1, not 0, when `contents:` held only
            // negations and `resolve_content_globs` injected a
            // synthesized default positive pattern: that pattern has
            // no declared position of its own, stands for the
            // implicit "everything", and is defined to precede every
            // declared entry (team-lead ruling, task 5 round 2) — so
            // a record declared after the negation sorts after the
            // default's items, not before them.
            let mut globs_before = usize::from(resolution.injected_default_positive);
            let mut raw_ordinal = 0usize;
            for entry in &contents {
                let value = match entry {
                    ListingContents::Glob { .. } => {
                        if let Some(Some(glob_idx)) = resolution.entry_index.get(raw_ordinal)
                            && let Some(ordinal) = positive_ordinal_by_glob_idx[*glob_idx]
                        {
                            globs_before = ordinal + 1;
                        }
                        raw_ordinal += 1;
                        continue;
                    }
                    ListingContents::Inline(value) => value,
                };
                let rec = parse_record(value, &mut diags);
                let base_dir = base_ctx.base_dir_for(&rec.source);
                let item = match rec.path.clone() {
                    None => record_item(rec, ItemTarget::None, &base_dir),
                    Some((raw, path_source)) => match resolve_record_path(
                        &raw,
                        &path_source,
                        &base_ctx,
                        ctx.project_index.as_deref(),
                        &mut diags,
                    ) {
                        RecordPath::Document(profile) => {
                            if !item_visible(profile) {
                                continue;
                            }
                            overlay_record(hydrate_item(profile), rec, &base_dir)
                        }
                        RecordPath::Href(href) => {
                            record_item(rec, ItemTarget::Href(href), &base_dir)
                        }
                    },
                };
                ordered.push(((globs_before, 0), item));
            }
            ordered.sort_by_key(|(key, _)| *key);
            let mut items: Vec<ListingItem> = ordered.into_iter().map(|(_, item)| item).collect();

            // A pattern that compiled, stayed in the project, and
            // still matched no document is almost always a Q1
            // assumption about `*` (D5/D7). Checked before
            // `include:`/`exclude:` filters run, so this reports on
            // the *glob*, not on a filter that emptied the set.
            for (i, (glob, source)) in positives.iter().enumerate() {
                if !matched_any[i] {
                    diags.push(
                        crate::glob::diagnostics::matched_nothing(
                            "Q-12-19",
                            "Listing `contents:`",
                            &glob.pattern,
                            source,
                        )
                        .add_info(
                            "Patterns resolve against the directory of the file they are \
                             written in; a leading `/` anchors at the project root.",
                        )
                        .build(),
                    );
                }
            }

            apply_filters(&mut items, &listing.include, &listing.exclude);

            if let Some(sort) = listing.sort.as_ref() {
                // `sort: false` parses to an empty spec, which
                // `apply_sort` treats as a no-op — declared
                // `contents:` order flows through untouched.
                apply_sort(&mut items, sort, &mut diags);
            } else {
                // Default sort (Q1 parity): `order asc, title asc`,
                // uniformly across listing types — Q1 applies its
                // default whenever `title` is among the hydrated
                // fields, which holds for every built-in type
                // (table included). Items without `order:` sort
                // after curated ones, in title order
                // (bd-listing-declared-order-3ixcvc4o).
                use crate::project::listing::config::{ListingSort, SortDirection};
                let default_sort = vec![
                    ListingSort {
                        field: "order".to_string(),
                        direction: SortDirection::Asc,
                    },
                    ListingSort {
                        field: "title".to_string(),
                        direction: SortDirection::Asc,
                    },
                ];
                apply_sort(&mut items, &default_sort, &mut diags);
            }

            if let Some(max) = listing.max_items {
                items.truncate(max as usize);
            }

            // bd-qv2lsab0: an author-supplied front-matter `image:`
            // is typically referenced by no page body, so nothing
            // else copies it into the output tree. Register a copy
            // intent per project-relative item image; they flush
            // with the host page's other copies.
            for item in &items {
                if let Some(img) = item.image.as_deref()
                    && !crate::project::listing::helpers::is_external_src(img)
                {
                    ctx.resource_copies.push(crate::render::ResourceCopyIntent {
                        src: ctx.project.dir.join(img),
                        dest: ctx.project.output_dir.join(img),
                        origin: quarto_source_map::SourceInfo::generated(
                            quarto_source_map::By::programmatic_config(),
                        ),
                    });
                }
            }

            resolved.push(ResolvedListing { listing, items });
        }

        ctx.resolved_listings = resolved;
        ctx.diagnostics = diags;
        Ok(())
    }
}

/// Whether a document may appear as a listing item.
///
/// Listings do not filter drafts today (Q1 does). bd-zeormbsa
/// introduces the shared `is_linkable` predicate on `ProjectIndex`;
/// this is the one seam it replaces — the glob path and the record
/// `path:` path both go through it, so the two can never disagree.
fn item_visible(_profile: &DocumentProfile) -> bool {
    true
}

enum RecordPath<'a> {
    Document(&'a DocumentProfile),
    Href(String),
}

/// Resolve a record's `path:` (Q1 `listItemFromMeta`, plan §D4).
fn resolve_record_path<'a>(
    raw: &str,
    source: &SourceInfo,
    base_ctx: &BaseDirContext<'_>,
    index: Option<&'a ProjectIndex>,
    diags: &mut Vec<DiagnosticMessage>,
) -> RecordPath<'a> {
    // Remote only — a leading `/` is the *project root* here and must
    // fall through to `join_and_normalize` (plan §D4).
    if is_remote_src(raw) {
        return RecordPath::Href(raw.to_string());
    }
    let base_dir = base_ctx.base_dir_for(source);
    let Some(resolved) = join_and_normalize(&base_dir, raw) else {
        diags.push(
            crate::glob::diagnostics::escapes_project("Q-12-17", "Listing record", raw, source)
                .build(),
        );
        return RecordPath::Href(raw.to_string());
    };
    if !is_markdown_document_path(&resolved) {
        return RecordPath::Href(raw.to_string());
    }
    match index.and_then(|i| i.lookup_by_source(std::path::Path::new(&resolved))) {
        Some(profile) => RecordPath::Document(profile),
        None => {
            diags.push(record_path_not_found(
                raw, &resolved, &base_dir, source, index,
            ));
            RecordPath::Href(raw.to_string())
        }
    }
}

fn record_path_not_found(
    raw: &str,
    resolved: &str,
    base_dir: &str,
    source: &SourceInfo,
    index: Option<&ProjectIndex>,
) -> DiagnosticMessage {
    let against = if base_dir.is_empty() {
        "the project root".to_string()
    } else {
        format!("`{base_dir}/`")
    };
    let mut b = DiagnosticMessageBuilder::warning(format!(
        "Listing record `path: {raw}` names no project document"
    ))
    .with_code("Q-12-20")
    .with_location(source.clone())
    .problem(format!(
        "Resolved to `{resolved}` (relative to {against}, where the listing is declared), \
         which is not a document this project renders. The item keeps the link as written, \
         so it may be broken."
    ))
    .add_info(
        "Paths resolve against the directory of the file the listing is written in; \
         a leading `/` anchors at the project root.",
    );
    let want = std::path::Path::new(resolved)
        .file_name()
        .and_then(|f| f.to_str());
    if let Some(candidate) = index.and_then(|i| {
        i.profiles()
            .iter()
            .find(|p| p.source_path.file_name().and_then(|f| f.to_str()) == want)
            .map(|p| path_to_forward_slashes(&p.source_path))
    }) {
        b = b.add_hint(format!("Did you mean `{candidate}`?"));
    }
    b.build()
}

fn yaml_contents_unsupported(pattern: &str, source: &SourceInfo) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(format!(
        "Listing `contents:` entry `{pattern}` is a YAML file, which is not supported yet"
    ))
    .with_code("Q-12-23")
    .with_location(source.clone())
    .problem(
        "Quarto 1 reads a YAML file in `contents:` as a list of listing records. Quarto 2 \
         does not yet (tracked as bd-hj1ehfn8); the entry is skipped.",
    )
    .add_hint("Move the records inline under `contents:` — each `- title: …` map becomes one item.")
    .build()
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

    fn s(value: &str) -> ConfigValue {
        ConfigValue::new_string(value, SourceInfo::for_test())
    }

    fn b(value: bool) -> ConfigValue {
        ConfigValue::new_bool(value, SourceInfo::for_test())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
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

    fn make_profile(source: &str, output_href: &str, title: &str) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: output_href.to_string(),
            format_id: "html".to_string(),
            title: Some(title.to_string()),
            ..DocumentProfile::default()
        }
    }

    fn make_profile_with_date(
        source: &str,
        output_href: &str,
        title: &str,
        date: &str,
    ) -> DocumentProfile {
        let mut p = make_profile(source, output_href, title);
        p.date = Some(date.to_string());
        p
    }

    fn make_profile_with_order(
        source: &str,
        output_href: &str,
        title: &str,
        order: i32,
    ) -> DocumentProfile {
        let mut p = make_profile(source, output_href, title);
        p.order = Some(order);
        p
    }

    fn titles(resolved: &[ResolvedListing]) -> Vec<&str> {
        resolved[0].items.iter().map(|i| i.title.as_str()).collect()
    }

    fn make_project(
        host: &str,
        profiles: Vec<DocumentProfile>,
    ) -> (ProjectContext, Arc<ProjectIndex>) {
        let _ = host;
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: profiles
                .iter()
                .map(|p| DocumentInfo::from_path(format!("/project/{}", p.source_path.display())))
                .collect(),
            output_dir: PathBuf::from("/project/_site"),

            ..Default::default()
        };
        let index = Arc::new(ProjectIndex::new(profiles));
        (project, index)
    }

    async fn run_transform(
        meta: ConfigValue,
        host_path: &str,
        profiles: Vec<DocumentProfile>,
    ) -> (
        Vec<ResolvedListing>,
        Vec<quarto_error_reporting::DiagnosticMessage>,
    ) {
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let (project, index) = make_project(host_path, profiles);
        let doc = DocumentInfo::from_path(format!("/project/{}", host_path));
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx =
            RenderContext::new(&project, &doc, &format, &binaries).with_project_index(index);
        ListingGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ctx.resolved_listings, ctx.diagnostics)
    }

    // 28. generate_skips_when_no_listing_key
    #[tokio::test]
    async fn generate_skips_when_no_listing_key() {
        let (resolved, diags) = run_transform(
            ConfigValue::default(),
            "index.qmd",
            vec![make_profile("posts/a.qmd", "posts/a.html", "A")],
        )
        .await;
        assert!(resolved.is_empty());
        assert!(diags.is_empty());
    }

    // 28b. generate_skips_when_listing_false
    #[tokio::test]
    async fn generate_skips_when_listing_false() {
        let (resolved, diags) =
            run_transform(map(vec![("listing", b(false))]), "index.qmd", vec![]).await;
        assert!(resolved.is_empty());
        // is_feature_disabled short-circuits before parse_listings,
        // so no Q-12-6 diag is emitted on this path.
        assert!(diags.is_empty());
    }

    // 29. generate_writes_resolved_listing — host page +
    //     three sibling posts → resolved listing has 3 items.
    //
    // Q1's default `*.qmd` is host-dir-relative, so we put the
    // host inside `posts/` to find sibling .qmd files.
    #[tokio::test]
    async fn generate_writes_resolved_listing() {
        let (resolved, diags) = run_transform(
            map(vec![("listing", s("default"))]),
            "posts/index.qmd",
            vec![
                make_profile_with_date("posts/a.qmd", "posts/a.html", "A", "2026-01-01"),
                make_profile_with_date("posts/b.qmd", "posts/b.html", "B", "2026-02-01"),
                make_profile_with_date("posts/c.qmd", "posts/c.html", "C", "2026-03-01"),
                // Host page itself — must be excluded.
                make_profile("posts/index.qmd", "posts/index.html", "Home"),
            ],
        )
        .await;
        assert!(diags.is_empty(), "diags: {:?}", diags);
        assert_eq!(resolved.len(), 1);
        // Three posts (no host).
        assert_eq!(resolved[0].items.len(), 3);
    }

    // 30. generate_filters_via_include
    #[tokio::test]
    async fn generate_filters_via_include() {
        let (resolved, _) = run_transform(
            map(vec![(
                "listing",
                map(vec![
                    ("type", s("default")),
                    ("include", arr(vec![map(vec![("title", s("Keep"))])])),
                ]),
            )]),
            "posts/index.qmd",
            vec![
                make_profile("posts/a.qmd", "posts/a.html", "Keep"),
                make_profile("posts/b.qmd", "posts/b.html", "Drop"),
            ],
        )
        .await;
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].items.len(), 1);
        assert_eq!(resolved[0].items[0].title, "Keep");
    }

    // 31. generate_sorts_via_sort_field — date desc explicit.
    #[tokio::test]
    async fn generate_sorts_via_explicit_sort_field() {
        let (resolved, _) = run_transform(
            map(vec![(
                "listing",
                map(vec![
                    ("type", s("default")),
                    ("sort", arr(vec![s("date desc")])),
                ]),
            )]),
            "posts/index.qmd",
            vec![
                make_profile_with_date("posts/a.qmd", "posts/a.html", "A", "2026-01-01"),
                make_profile_with_date("posts/c.qmd", "posts/c.html", "C", "2026-03-01"),
                make_profile_with_date("posts/b.qmd", "posts/b.html", "B", "2026-02-01"),
            ],
        )
        .await;
        assert_eq!(resolved[0].items.len(), 3);
        // Newest first.
        assert_eq!(resolved[0].items[0].title, "C");
        assert_eq!(resolved[0].items[1].title, "B");
        assert_eq!(resolved[0].items[2].title, "A");
    }

    // 32. generate_excludes_host_page_itself — covered in #29 above
    // (we explicitly include the host in the project index and assert
    // it doesn't appear).

    // 32b. host-relative glob default (`*.qmd`) finds siblings, not
    //      nested-dir files — Q1 default behavior.
    #[tokio::test]
    async fn default_glob_matches_host_dir_siblings_only() {
        let (resolved, _) = run_transform(
            map(vec![("listing", s("default"))]),
            "posts/index.qmd",
            vec![
                make_profile("posts/a.qmd", "posts/a.html", "A"),
                make_profile("posts/b.qmd", "posts/b.html", "B"),
                // Different directory — should not match `*.qmd`
                // when the listing is on `posts/index.qmd`.
                make_profile("notes/c.qmd", "notes/c.html", "C"),
                // Host itself — excluded.
                make_profile("posts/index.qmd", "posts/index.html", "Home"),
            ],
        )
        .await;
        assert_eq!(resolved[0].items.len(), 2);
        let titles: Vec<&str> = resolved[0].items.iter().map(|i| i.title.as_str()).collect();
        assert!(titles.contains(&"A"));
        assert!(titles.contains(&"B"));
        assert!(!titles.contains(&"C"));
    }

    // Project-relative explicit glob from a host that's at root.
    #[tokio::test]
    async fn project_relative_glob_matches_files_in_subdir() {
        let (resolved, _) = run_transform(
            map(vec![(
                "listing",
                map(vec![
                    ("type", s("default")),
                    ("contents", arr(vec![s("posts/**/*.qmd")])),
                ]),
            )]),
            "index.qmd",
            vec![
                make_profile("posts/a.qmd", "posts/a.html", "A"),
                make_profile("posts/sub/b.qmd", "posts/sub/b.html", "B"),
                make_profile("notes/c.qmd", "notes/c.html", "C"),
            ],
        )
        .await;
        assert_eq!(resolved[0].items.len(), 2);
        let titles: Vec<&str> = resolved[0].items.iter().map(|i| i.title.as_str()).collect();
        assert!(titles.contains(&"A"));
        assert!(titles.contains(&"B"));
    }

    #[tokio::test]
    async fn max_items_truncates_after_sort() {
        let (resolved, _) = run_transform(
            map(vec![(
                "listing",
                map(vec![
                    ("type", s("default")),
                    ("max-items", s("2")),
                    ("sort", arr(vec![s("date desc")])),
                ]),
            )]),
            "posts/index.qmd",
            vec![
                make_profile_with_date("posts/a.qmd", "posts/a.html", "A", "2026-01-01"),
                make_profile_with_date("posts/b.qmd", "posts/b.html", "B", "2026-02-01"),
                make_profile_with_date("posts/c.qmd", "posts/c.html", "C", "2026-03-01"),
            ],
        )
        .await;
        assert_eq!(resolved[0].items.len(), 2);
        // Sort by date desc + truncate → C, B (A drops off).
        assert_eq!(resolved[0].items[0].title, "C");
        assert_eq!(resolved[0].items[1].title, "B");
    }

    // Q1 parity (bd-listing-declared-order-3ixcvc4o): absent `sort:`
    // applies `order asc, title asc` — NOT date desc. Every item
    // carries a date, and the dates contradict both the order: values
    // and the titles, so a date-driven default cannot produce the
    // expected sequence.
    #[tokio::test]
    async fn default_sort_is_order_asc_then_title_asc() {
        let with_order = |source: &str, href: &str, title: &str, date: &str, order: i32| {
            let mut p = make_profile_with_date(source, href, title, date);
            p.order = Some(order);
            p
        };
        let (resolved, _) = run_transform(
            map(vec![("listing", s("default"))]),
            "posts/index.qmd",
            vec![
                make_profile_with_date("posts/a.qmd", "posts/a.html", "Delta", "2026-04-01"),
                with_order("posts/b.qmd", "posts/b.html", "Zulu", "2026-01-01", 1),
                make_profile_with_date("posts/c.qmd", "posts/c.html", "Alpha", "2026-02-01"),
                with_order("posts/d.qmd", "posts/d.html", "Mike", "2026-03-01", 2),
            ],
        )
        .await;
        // order: 1 first, order: 2 second, then order-less items by
        // title asc (missing order values sort last).
        assert_eq!(titles(&resolved), ["Zulu", "Mike", "Alpha", "Delta"]);
    }

    // The default sort is uniform across listing types (Q1 applies it
    // whenever `title` is among the hydrated fields, which holds for
    // every built-in type — including table).
    #[tokio::test]
    async fn table_type_gets_default_sort_too() {
        let (resolved, _) = run_transform(
            map(vec![("listing", map(vec![("type", s("table"))]))]),
            "posts/index.qmd",
            vec![
                make_profile_with_order("posts/a.qmd", "posts/a.html", "Second", 2),
                make_profile_with_order("posts/b.qmd", "posts/b.html", "First", 1),
            ],
        )
        .await;
        assert_eq!(titles(&resolved), ["First", "Second"]);
    }

    // `sort: true` is Q1's "apply the default sort" — identical to an
    // absent `sort:` key, and NOT a sort by a field named "true".
    #[tokio::test]
    async fn sort_true_behaves_like_absent_sort() {
        let (resolved, diags) = run_transform(
            map(vec![(
                "listing",
                map(vec![("type", s("default")), ("sort", b(true))]),
            )]),
            "posts/index.qmd",
            vec![
                make_profile_with_order("posts/a.qmd", "posts/a.html", "Second", 2),
                make_profile_with_order("posts/b.qmd", "posts/b.html", "First", 1),
            ],
        )
        .await;
        assert!(
            !diags.iter().any(|d| d.code.as_deref() == Some("Q-12-3")),
            "sort: true must not diagnose; got {:?}",
            diags
        );
        assert_eq!(titles(&resolved), ["First", "Second"]);
    }

    // bd-listing-declared-order-3ixcvc4o: with `sort: false`, explicit
    // `contents:` entries render in declared order (Q1 semantics), not
    // in project-index order.
    #[tokio::test]
    async fn sort_false_preserves_declared_contents_order() {
        let (resolved, _) = run_transform(
            map(vec![(
                "listing",
                map(vec![
                    ("type", s("default")),
                    ("sort", b(false)),
                    ("contents", arr(vec![s("bravo.qmd"), s("alpha.qmd")])),
                ]),
            )]),
            "index.qmd",
            vec![
                // Index order is alphabetical — the opposite of the
                // declared order — to prove the reordering happens.
                make_profile("alpha.qmd", "alpha.html", "Alpha"),
                make_profile("bravo.qmd", "bravo.html", "Bravo"),
            ],
        )
        .await;
        assert_eq!(titles(&resolved), ["Bravo", "Alpha"]);
    }

    // Q1's rule generalizes to wildcard patterns: items are ordered by
    // the index of the first pattern that matches them; within one
    // pattern, project-index order.
    #[tokio::test]
    async fn contents_ordered_by_first_matching_pattern_index() {
        let (resolved, _) = run_transform(
            map(vec![(
                "listing",
                map(vec![
                    ("type", s("default")),
                    ("sort", b(false)),
                    ("contents", arr(vec![s("z.qmd"), s("a*.qmd")])),
                ]),
            )]),
            "index.qmd",
            vec![
                make_profile("a1.qmd", "a1.html", "A1"),
                make_profile("a2.qmd", "a2.html", "A2"),
                make_profile("z.qmd", "z.html", "Zed"),
            ],
        )
        .await;
        assert_eq!(titles(&resolved), ["Zed", "A1", "A2"]);
    }

    // An item matched by several patterns belongs to the FIRST one and
    // appears exactly once.
    #[tokio::test]
    async fn item_matching_multiple_patterns_appears_once_at_first_pattern() {
        let (resolved, _) = run_transform(
            map(vec![(
                "listing",
                map(vec![
                    ("type", s("default")),
                    ("sort", b(false)),
                    ("contents", arr(vec![s("b.qmd"), s("*.qmd")])),
                ]),
            )]),
            "index.qmd",
            vec![
                make_profile("a.qmd", "a.html", "Aye"),
                make_profile("b.qmd", "b.html", "Bee"),
            ],
        )
        .await;
        assert_eq!(titles(&resolved), ["Bee", "Aye"]);
    }

    // Q-12-19 ("matched nothing") must credit EVERY pattern an item
    // matches, not just the first-match winner: `b*.qmd`'s only match
    // is claimed by `b.qmd` for ordering purposes, but it still
    // matched something.
    #[tokio::test]
    async fn q_12_19_silent_when_matches_claimed_by_earlier_pattern() {
        let (resolved, diags) = run_transform(
            map(vec![(
                "listing",
                map(vec![
                    ("type", s("default")),
                    ("sort", b(false)),
                    ("contents", arr(vec![s("b.qmd"), s("b*.qmd")])),
                ]),
            )]),
            "index.qmd",
            vec![make_profile("b.qmd", "b.html", "Bee")],
        )
        .await;
        assert_eq!(titles(&resolved), ["Bee"]);
        assert!(
            !diags.iter().any(|d| d.code.as_deref() == Some("Q-12-19")),
            "no matched-nothing diag expected; got {:?}",
            diags
        );
    }

    #[tokio::test]
    async fn pre_populated_resolved_listings_are_treated_as_override() {
        // If something earlier already populated resolved_listings,
        // we treat it as an override and bail out (mirrors navbar's
        // skip-when-already-set rule).
        use crate::project::listing::Listing;

        let mut ast = Pandoc {
            meta: map(vec![("listing", s("default"))]),
            blocks: vec![],
        };
        let (project, index) = make_project(
            "index.qmd",
            vec![make_profile("posts/a.qmd", "posts/a.html", "A")],
        );
        let doc = DocumentInfo::from_path("/project/index.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx =
            RenderContext::new(&project, &doc, &format, &binaries).with_project_index(index);
        ctx.resolved_listings = vec![ResolvedListing {
            listing: Listing::default(),
            items: vec![],
        }];

        ListingGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        // The pre-populated empty resolved listing wasn't overwritten.
        assert_eq!(ctx.resolved_listings.len(), 1);
        assert!(ctx.resolved_listings[0].items.is_empty());
    }

    // bd-listing-inline-contents-tyy446ze Task 5: records in the
    // generate transform.
    use crate::project::listing::{ItemOrigin, ItemTarget};

    fn contents_listing(entries: Vec<ConfigValue>) -> ConfigValue {
        map(vec![(
            "listing",
            map(vec![("id", s("l")), ("contents", arr(entries))]),
        )])
    }
    fn contents_listing_unsorted(entries: Vec<ConfigValue>) -> ConfigValue {
        map(vec![(
            "listing",
            map(vec![
                ("id", s("l")),
                ("sort", b(false)),
                ("contents", arr(entries)),
            ]),
        )])
    }
    fn codes(diags: &[quarto_error_reporting::DiagnosticMessage]) -> Vec<&str> {
        diags.iter().filter_map(|d| d.code.as_deref()).collect()
    }

    #[tokio::test]
    async fn record_without_path_becomes_unlinked_item_with_custom_fields() {
        let (resolved, diags) = run_transform(
            contents_listing(vec![map(vec![
                ("title", s("Get started")),
                ("icon", s("bi-rocket-takeoff")),
                ("link", s("download.qmd")),
            ])]),
            "index.qmd",
            vec![make_profile("index.qmd", "index.html", "Home")],
        )
        .await;
        assert!(diags.is_empty(), "{diags:?}");
        let item = &resolved[0].items[0];
        assert_eq!(item.title, "Get started");
        assert_eq!(item.target, ItemTarget::None);
        assert_eq!(item.origin, ItemOrigin::Record);
        assert_eq!(
            item.extra
                .get("link")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("download.qmd")
        );
    }

    #[tokio::test]
    async fn record_path_overlays_the_named_document() {
        let mut doc = make_profile("download.qmd", "download.html", "Download stub");
        doc.description = Some("from the document".to_string());
        let (resolved, diags) = run_transform(
            contents_listing(vec![map(vec![
                ("title", s("Get started")),
                ("path", s("download.qmd")),
            ])]),
            "index.qmd",
            vec![make_profile("index.qmd", "index.html", "Home"), doc],
        )
        .await;
        assert!(diags.is_empty(), "{diags:?}");
        let item = &resolved[0].items[0];
        assert_eq!(item.title, "Get started");
        assert_eq!(item.description.as_deref(), Some("from the document"));
        assert_eq!(
            item.target,
            ItemTarget::document("download.qmd", "download.html")
        );
        assert_eq!(item.origin, ItemOrigin::RecordOverDocument);
    }

    #[tokio::test]
    async fn record_path_resolves_against_the_host_directory() {
        let (resolved, diags) = run_transform(
            contents_listing(vec![map(vec![("path", s("../rootpost.qmd"))])]),
            "sub/index.qmd",
            vec![
                make_profile("sub/index.qmd", "sub/index.html", "Sub"),
                make_profile("rootpost.qmd", "rootpost.html", "Root Post"),
            ],
        )
        .await;
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(resolved[0].items[0].title, "Root Post");
        assert_eq!(
            resolved[0].items[0].target,
            ItemTarget::document("rootpost.qmd", "rootpost.html")
        );
    }

    #[tokio::test]
    async fn record_path_to_unknown_document_warns_q_12_20_and_keeps_href() {
        let (resolved, diags) = run_transform(
            contents_listing(vec![map(vec![
                ("title", s("Typo")),
                ("path", s("downlaod.qmd")),
            ])]),
            "index.qmd",
            vec![
                make_profile("index.qmd", "index.html", "Home"),
                make_profile("guide/downlaod.qmd", "guide/downlaod.html", "Elsewhere"),
            ],
        )
        .await;
        assert_eq!(codes(&diags), vec!["Q-12-20"]);
        let hint = format!("{:?}", diags[0]);
        assert!(
            hint.contains("guide/downlaod.qmd"),
            "did-you-mean names the same-named document: {hint}"
        );
        assert_eq!(
            resolved[0].items[0].target,
            ItemTarget::Href("downlaod.qmd".to_string())
        );
        assert_eq!(resolved[0].items[0].title, "Typo");
    }

    #[tokio::test]
    async fn record_path_external_url_and_non_document_are_literal_hrefs() {
        let (resolved, diags) = run_transform(
            contents_listing_unsorted(vec![
                map(vec![
                    ("title", s("Site")),
                    ("path", s("https://example.com/")),
                ]),
                map(vec![
                    ("title", s("Report")),
                    ("path", s("files/report.pdf")),
                ]),
            ]),
            "index.qmd",
            vec![make_profile("index.qmd", "index.html", "Home")],
        )
        .await;
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            resolved[0].items[0].target,
            ItemTarget::Href("https://example.com/".to_string())
        );
        assert_eq!(
            resolved[0].items[1].target,
            ItemTarget::Href("files/report.pdf".to_string())
        );
    }

    #[tokio::test]
    async fn record_path_with_leading_slash_anchors_at_the_project_root() {
        let (resolved, diags) = run_transform(
            contents_listing(vec![map(vec![("path", s("/rootpost.qmd"))])]),
            "sub/index.qmd",
            vec![
                make_profile("sub/index.qmd", "sub/index.html", "Sub"),
                make_profile("rootpost.qmd", "rootpost.html", "Root Post"),
            ],
        )
        .await;
        assert!(
            diags.is_empty(),
            "a leading `/` is the project root, not a remote URL; {diags:?}"
        );
        assert_eq!(
            resolved[0].items[0].target,
            ItemTarget::document("rootpost.qmd", "rootpost.html")
        );
    }

    #[tokio::test]
    async fn record_path_escaping_the_project_warns_q_12_17() {
        let (resolved, diags) = run_transform(
            contents_listing(vec![map(vec![
                ("title", s("Out")),
                ("path", s("../../x.qmd")),
            ])]),
            "index.qmd",
            vec![make_profile("index.qmd", "index.html", "Home")],
        )
        .await;
        assert_eq!(codes(&diags), vec!["Q-12-17"]);
        assert_eq!(
            resolved[0].items[0].target,
            ItemTarget::Href("../../x.qmd".to_string())
        );
    }

    #[tokio::test]
    async fn records_keep_their_declared_position_under_sort_false() {
        let (resolved, diags) = run_transform(
            contents_listing_unsorted(vec![
                map(vec![("title", s("First record"))]),
                s("posts/*.qmd"),
                map(vec![("title", s("Last record"))]),
            ]),
            "index.qmd",
            vec![
                make_profile("index.qmd", "index.html", "Home"),
                make_profile("posts/a.qmd", "posts/a.html", "A"),
            ],
        )
        .await;
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(titles(&resolved), vec!["First record", "A", "Last record"]);
    }

    #[tokio::test]
    async fn record_and_glob_naming_the_same_document_yield_two_items() {
        let (resolved, _) = run_transform(
            contents_listing_unsorted(vec![
                map(vec![("title", s("Featured")), ("path", s("posts/a.qmd"))]),
                s("posts/*.qmd"),
            ]),
            "index.qmd",
            vec![
                make_profile("index.qmd", "index.html", "Home"),
                make_profile("posts/a.qmd", "posts/a.html", "A"),
            ],
        )
        .await;
        assert_eq!(
            titles(&resolved),
            vec!["Featured", "A"],
            "Q1 parity: no dedupe"
        );
    }

    #[tokio::test]
    async fn yaml_file_entry_warns_q_12_23_and_not_q_12_19() {
        let (resolved, diags) = run_transform(
            contents_listing(vec![s("items.yml"), s("posts/*.qmd")]),
            "index.qmd",
            vec![
                make_profile("index.qmd", "index.html", "Home"),
                make_profile("posts/a.qmd", "posts/a.html", "A"),
            ],
        )
        .await;
        assert_eq!(codes(&diags), vec!["Q-12-23"]);
        assert_eq!(titles(&resolved), vec!["A"]);
    }

    #[tokio::test]
    async fn record_near_miss_and_missing_title_surface_from_generate() {
        let (_, diags) = run_transform(
            contents_listing(vec![
                map(vec![("titel", s("x"))]),
                map(vec![("description", s("no title"))]),
            ]),
            "index.qmd",
            vec![make_profile("index.qmd", "index.html", "Home")],
        )
        .await;
        let mut got = codes(&diags);
        got.sort_unstable();
        assert_eq!(got, vec!["Q-12-21", "Q-12-21", "Q-12-22"]);
    }

    // Review fix (bd-listing-inline-contents-tyy446ze, task 5 round 1):
    // a record's declared-position key must count only Glob entries
    // that actually contribute a positive pattern to
    // `resolution.positives()` — a negated pattern before the record
    // must not advance its key, or the record sorts after a later
    // positive glob's items under `sort: false`.
    #[tokio::test]
    async fn record_keeps_position_when_a_negated_pattern_precedes_it() {
        let (resolved, diags) = run_transform(
            contents_listing_unsorted(vec![
                s("!posts/draft.qmd"),
                map(vec![("title", s("Featured"))]),
                s("posts/*.qmd"),
            ]),
            "index.qmd",
            vec![
                make_profile("index.qmd", "index.html", "Home"),
                make_profile("posts/a.qmd", "posts/a.html", "A"),
            ],
        )
        .await;
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(titles(&resolved), vec!["Featured", "A"]);
    }

    // Same failure mode, different reason a leading pattern
    // contributes nothing to `resolution.positives()`: it escapes the
    // project root (Q-12-17) rather than being negated.
    #[tokio::test]
    async fn record_keeps_position_when_an_escaping_pattern_precedes_it() {
        let (resolved, diags) = run_transform(
            contents_listing_unsorted(vec![
                s("../../x.qmd"),
                map(vec![("title", s("Featured"))]),
                s("posts/*.qmd"),
            ]),
            "index.qmd",
            vec![
                make_profile("index.qmd", "index.html", "Home"),
                make_profile("posts/a.qmd", "posts/a.html", "A"),
            ],
        )
        .await;
        assert_eq!(codes(&diags), vec!["Q-12-17"]);
        assert_eq!(titles(&resolved), vec!["Featured", "A"]);
    }

    // Review fix (bd-listing-inline-contents-tyy446ze, task 5 round
    // 2): a `contents:` that is only negations triggers
    // `resolve_content_globs`'s injected default positive (Q1
    // parity: the host directory's `*.qmd` siblings). The synthesized
    // pattern has no declared position of its own — team-lead ruling:
    // it stands for the implicit "everything" and is defined to
    // precede every declared entry, so a record declared after the
    // negation sorts *after* the default's items, not before them.
    #[tokio::test]
    async fn record_after_negation_only_prefix_sorts_after_the_injected_default() {
        let (resolved, diags) = run_transform(
            contents_listing_unsorted(vec![s("!draft.qmd"), map(vec![("title", s("Featured"))])]),
            "index.qmd",
            vec![
                make_profile("index.qmd", "index.html", "Home"),
                make_profile("a.qmd", "a.html", "A"),
                make_profile("draft.qmd", "draft.html", "Draft"),
            ],
        )
        .await;
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(titles(&resolved), vec!["A", "Featured"]);
    }

    // Review fix (task 5 round 2): the round-1 text-matching
    // reconstruction misclassified a record's position when the SAME
    // raw pattern text appeared twice with different provenance —
    // once resolving fine (declared in `sub/index.qmd`, base dir
    // `sub`), once escaping the project root (declared in
    // `_quarto.yml`, base dir the project root). `take_first` matched
    // the *first* occurrence regardless of which one actually
    // escaped, crediting the escape to the wrong entry and letting
    // the record jump ahead of an item it was written after. The
    // structural fix (`GlobResolution::entry_index`) can't confuse
    // the two: each raw entry's own outcome is recorded at resolution
    // time, not reconstructed afterward by matching text.
    #[tokio::test]
    async fn record_position_unaffected_by_duplicate_pattern_text_with_different_provenance() {
        use quarto_source_map::SourceContext;

        let mut sc = SourceContext::new();
        let sub_id = sc.add_file("/project/sub/index.qmd".to_string(), None);
        let root_id = sc.add_file("/project/_quarto.yml".to_string(), None);
        // Base dir "sub" — "../x.qmd" normalizes to "x.qmd" (valid).
        let valid_source = SourceInfo::original(sub_id, 0, 10);
        // Base dir "" (project root) — "../x.qmd" escapes.
        let escaped_source = SourceInfo::original(root_id, 0, 10);

        let contents = arr(vec![
            ConfigValue::new_string("../x.qmd", valid_source),
            map(vec![("title", s("Featured"))]),
            ConfigValue::new_string("../x.qmd", escaped_source),
        ]);
        let meta = map(vec![(
            "listing",
            map(vec![
                ("id", s("l")),
                ("sort", b(false)),
                ("contents", contents),
            ]),
        )]);

        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let (project, index) = make_project(
            "sub/index.qmd",
            vec![
                make_profile("sub/index.qmd", "sub/index.html", "Sub"),
                make_profile("x.qmd", "x.html", "X"),
            ],
        );
        let doc = DocumentInfo::from_path("/project/sub/index.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx =
            RenderContext::new(&project, &doc, &format, &binaries).with_project_index(index);
        ctx.source_context = Some(&sc);

        ListingGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        assert_eq!(codes(&ctx.diagnostics), vec!["Q-12-17"]);
        // The valid "../x.qmd" (declared first) resolves to "x.qmd",
        // the project's only positive pattern — its item sorts before
        // the record declared after it; the escaping duplicate
        // (declared last) contributes nothing.
        assert_eq!(titles(&ctx.resolved_listings), vec!["X", "Featured"]);
    }
}

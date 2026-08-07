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

use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::project::discovery::path_to_forward_slashes;
use crate::project::listing::filter::apply_filters;
use crate::project::listing::glob_resolve::{item_matches, resolve_content_globs};
use crate::project::listing::sort::apply_sort;
use crate::project::listing::{ResolvedListing, hydrate_item, parse_listings};
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

        for listing in listings {
            // Resolve each glob against the directory of the file
            // it was written in (provenance-based; GH #456).
            let resolution = resolve_content_globs(
                &listing.contents,
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

            let mut items = Vec::new();
            if let Some(index) = ctx.project_index.as_deref() {
                for profile in index.profiles() {
                    let candidate_path_str = path_to_forward_slashes(&profile.source_path);
                    if candidate_path_str == host_path_str {
                        continue;
                    }
                    if item_matches(&resolution.globs, &candidate_path_str) {
                        items.push(hydrate_item(profile));
                    }
                }
            }

            apply_filters(&mut items, &listing.include, &listing.exclude);

            if let Some(sort) = listing.sort.as_ref() {
                apply_sort(&mut items, sort, &mut diags);
            } else {
                // Default sort: date descending. Matches Q1 default
                // for the `default` and `grid` types; `table` keeps
                // insertion order unless the author overrides.
                use crate::project::listing::config::{ListingSort, ListingType, SortDirection};
                if !matches!(listing.kind, ListingType::Table) {
                    let default_sort = vec![ListingSort {
                        field: "date".to_string(),
                        direction: SortDirection::Desc,
                    }];
                    apply_sort(&mut items, &default_sort, &mut diags);
                }
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

    #[tokio::test]
    async fn default_sort_is_date_desc_for_default_type() {
        let (resolved, _) = run_transform(
            map(vec![("listing", s("default"))]),
            "posts/index.qmd",
            vec![
                make_profile_with_date("posts/a.qmd", "posts/a.html", "A", "2026-01-01"),
                make_profile_with_date("posts/b.qmd", "posts/b.html", "B", "2026-03-01"),
                make_profile_with_date("posts/c.qmd", "posts/c.html", "C", "2026-02-01"),
            ],
        )
        .await;
        // No explicit sort → default = date desc.
        assert_eq!(resolved[0].items[0].title, "B");
        assert_eq!(resolved[0].items[1].title, "C");
        assert_eq!(resolved[0].items[2].title, "A");
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
}

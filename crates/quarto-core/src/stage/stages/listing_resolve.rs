/*
 * stage/stages/listing_resolve.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Listing resolution as a top-level pipeline stage (bd-2oqen84t, M2).
//!
//! Listings used to resolve inside `AstTransformsStage`, in the
//! `Navigation` phase (`ListingGenerateTransform`), long after
//! `lmd-execute` and `user-filters-pre` had run; so even with the
//! resolved set published at `meta.listings.<id>`
//! ([`crate::project::listing::metadata`]), no Lua code could see it.
//! Resolution needs only the Pass-1 project index and the host page's
//! own `listing:` metadata, neither of which an AST transform
//! produces, so it can run as soon as the engine has (its position in
//! `Navigation` was forward-compatibility for bd-0fd0, not a data
//! dependency).
//!
//! **Pipeline position:** after `attribution-generate`, immediately
//! before `lmd-execute`, so lmd cells and then the user pre-filters
//! both read `meta.listings.<id>`.
//!
//! The typed result rides on [`StageContext::resolved_listings`] into
//! `AstTransformsStage`, which bridges it into
//! `RenderContext::resolved_listings`; `ListingGenerateTransform` then
//! sees `listings_resolved` and skips, and `ListingRenderTransform`,
//! the categories sidebar and the feed consume the same set as before.
//! The render side stays where it is: it reads normalized metadata and
//! is followed by the link-rewrite and resource-collection transforms
//! that make item `path`s and images work.
//!
//! No-op when the page declares no `listing:` or has `listing: false`
//! under `disable`; in both cases `listings_resolved` stays `false`
//! and the transform behaves exactly as it did before this stage.

use async_trait::async_trait;

use crate::project::listing::metadata::publish_listings;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;
use crate::transforms::is_feature_disabled;
use crate::transforms::listing_generate::{ListingResolveInputs, resolve_listings};
use crate::transforms::navigation_active::page_relative_source_of;

/// Top-level stage that populates [`StageContext::resolved_listings`]
/// and `meta.listings.<id>`. See module docs.
pub struct ListingResolveStage;

impl ListingResolveStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListingResolveStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for ListingResolveStage {
    fn name(&self) -> &str {
        "listing-resolve"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        if is_feature_disabled(&doc.ast.meta, "listing") {
            return Ok(PipelineData::DocumentAst(doc));
        }
        let Some(listing_value) = doc.ast.meta.get("listing") else {
            return Ok(PipelineData::DocumentAst(doc));
        };
        // Same override rule as the transform: a resolved set someone
        // put on the context before this stage is theirs.
        if ctx.listings_resolved || !ctx.resolved_listings.is_empty() {
            return Ok(PipelineData::DocumentAst(doc));
        }

        let mut diags = Vec::new();
        let inputs = ListingResolveInputs {
            source_context: Some(&doc.source_context),
            project_dir: &ctx.project.dir,
            output_dir: &ctx.project.output_dir,
            project_index: ctx.project_index.as_deref(),
            host_path: page_relative_source_of(&ctx.document.input, &ctx.project.dir),
            vfs_root_mode: ctx
                .resource_resolver
                .as_ref()
                .is_some_and(|r| r.is_vfs_root_mode()),
        };
        let resolution = resolve_listings(listing_value, &inputs, &mut diags);

        trace_event!(
            ctx,
            EventLevel::Debug,
            "resolved {} listing(s) ({} item(s)) for meta.listings",
            resolution.resolved.len(),
            resolution
                .resolved
                .iter()
                .map(|r| r.items.len())
                .sum::<usize>()
        );

        ctx.resource_copies.extend(resolution.resource_copies);
        publish_listings(
            &mut doc.ast.meta,
            &resolution.resolved,
            &resolution.host_dir,
        );
        ctx.resolved_listings = resolution.resolved;
        ctx.listings_resolved = true;
        ctx.diagnostics.extend(diags);

        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::format::Format;
    use crate::project::index::ProjectIndex;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::stage::DocumentAst;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_source_map::SourceInfo;
    use quarto_system_runtime::NativeRuntime;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn s(value: &str) -> ConfigValue {
        ConfigValue::new_string(value, SourceInfo::for_test())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::for_test(),
                    value: v,
                })
                .collect(),
            SourceInfo::for_test(),
        )
    }

    fn profile(source: &str, title: &str, date: &str) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: source.replace(".qmd", ".html"),
            format_id: "html".to_string(),
            title: Some(crate::document_profile::text(title)),
            date: Some(date.to_string()),
            ..DocumentProfile::default()
        }
    }

    fn stage_ctx(host: &str, profiles: Vec<DocumentProfile>) -> StageContext {
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
        let doc = DocumentInfo::from_path(format!("/project/{host}"));
        let mut ctx =
            StageContext::new(Arc::new(NativeRuntime::new()), Format::html(), project, doc)
                .unwrap();
        ctx.project_index = Some(Arc::new(ProjectIndex::new(profiles)));
        ctx
    }

    fn doc_with_meta(host: &str, meta: ConfigValue) -> DocumentAst {
        DocumentAst {
            path: PathBuf::from(format!("/project/{host}")),
            ast: Pandoc {
                meta,
                blocks: vec![],
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: quarto_source_map::SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
            derived_listing: Default::default(),
        }
    }

    async fn run(
        host: &str,
        meta: ConfigValue,
        profiles: Vec<DocumentProfile>,
    ) -> (DocumentAst, StageContext) {
        let mut ctx = stage_ctx(host, profiles);
        let out = ListingResolveStage::new()
            .run(
                PipelineData::DocumentAst(doc_with_meta(host, meta)),
                &mut ctx,
            )
            .await
            .unwrap();
        let PipelineData::DocumentAst(doc) = out else {
            panic!("DocumentAst out")
        };
        (doc, ctx)
    }

    #[tokio::test]
    async fn resolves_and_publishes_before_the_transforms() {
        let meta = map(vec![(
            "listing",
            map(vec![
                ("id", s("posts")),
                ("contents", s("*.qmd")),
                ("sort", s("date desc")),
            ]),
        )]);
        let (doc, ctx) = run(
            "posts/index.qmd",
            meta,
            vec![
                profile("posts/a.qmd", "A", "2026-01-01"),
                profile("posts/b.qmd", "B", "2026-03-01"),
            ],
        )
        .await;
        assert!(ctx.listings_resolved);
        assert_eq!(ctx.resolved_listings.len(), 1);
        assert_eq!(ctx.resolved_listings[0].items.len(), 2);
        assert_eq!(ctx.resolved_listings[0].items[0].title_text(), "B");
        let items = doc
            .ast
            .meta
            .get_path(&["listings", "posts", "items"])
            .and_then(|v| v.as_array())
            .expect("meta.listings.posts.items");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get("path").unwrap().as_str(), Some("b.qmd"));
    }

    #[tokio::test]
    async fn no_listing_key_is_a_no_op() {
        let (doc, ctx) = run("index.qmd", ConfigValue::default(), vec![]).await;
        assert!(!ctx.listings_resolved);
        assert!(ctx.resolved_listings.is_empty());
        assert!(doc.ast.meta.get("listings").is_none());
    }

    #[tokio::test]
    async fn listing_false_is_a_no_op() {
        let meta = map(vec![(
            "listing",
            ConfigValue::new_bool(false, SourceInfo::for_test()),
        )]);
        let (doc, ctx) = run("index.qmd", meta, vec![profile("a.qmd", "A", "2026-01-01")]).await;
        assert!(!ctx.listings_resolved);
        assert!(doc.ast.meta.get("listings").is_none());
    }

    /// A `listing:` that parses to nothing still marks the page
    /// resolved, so the generate transform does not parse (and
    /// diagnose) it a second time.
    #[tokio::test]
    async fn unusable_listing_still_marks_the_page_resolved() {
        let meta = map(vec![(
            "listing",
            ConfigValue::new_bool(true, SourceInfo::for_test()),
        )]);
        let (_doc, ctx) = run("index.qmd", meta, vec![]).await;
        assert!(ctx.listings_resolved);
    }

    /// A resolved set already on the context is an override: the stage
    /// leaves it alone and publishes nothing.
    #[tokio::test]
    async fn pre_populated_context_is_an_override() {
        let meta = map(vec![("listing", map(vec![("contents", s("*.qmd"))]))]);
        let mut ctx = stage_ctx("index.qmd", vec![profile("a.qmd", "A", "2026-01-01")]);
        ctx.listings_resolved = true;
        let out = ListingResolveStage::new()
            .run(
                PipelineData::DocumentAst(doc_with_meta("index.qmd", meta)),
                &mut ctx,
            )
            .await
            .unwrap();
        let PipelineData::DocumentAst(doc) = out else {
            panic!("DocumentAst out")
        };
        assert!(ctx.resolved_listings.is_empty());
        assert!(doc.ast.meta.get("listings").is_none());
    }
}

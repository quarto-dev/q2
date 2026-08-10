/*
 * transforms/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Essential AST transforms for the render pipeline.
 */

//! Essential AST transforms for the render pipeline.
//!
//! This module contains the core transforms used in the Quarto render pipeline:
//!
//! - [`AppendixStructureTransform`] - Consolidates appendix content into single container
//! - [`CalloutTransform`] - Converts callout Divs to CustomNodes
//! - [`CalloutResolveTransform`] - Resolves Callout CustomNodes to standard Div structure
//! - [`FooterGenerateTransform`] - Resolves `page-footer:` YAML into `navigation.footer`
//! - [`FooterRenderTransform`] - Renders `navigation.footer` to HTML
//! - [`FootnotesTransform`] - Extracts footnotes and creates footnotes section
//! - [`MetadataNormalizeTransform`] - Normalizes document metadata (adds pagetitle, etc.)
//! - [`NavbarGenerateTransform`] - Resolves `navbar:` YAML into `navigation.navbar`
//! - [`NavbarRenderTransform`] - Renders `navigation.navbar` to HTML
//! - [`ResourceCollectorTransform`] - Collects resource dependencies (images, etc.)
//! - [`SectionizeTransform`] - Wraps headers in section Divs (analogous to Pandoc's --section-divs)
//! - [`ShortcodeResolveTransform`] - Resolves shortcodes to their content
//! - [`TitleBlockTransform`] - Adds title header from metadata if not present
//! - [`TocGenerateTransform`] - Generates TOC from document headings
//! - [`TocRenderTransform`] - Renders TOC metadata to HTML
//!
//! These transforms implement [`AstTransform`](crate::transform::AstTransform) and
//! can be added to a [`TransformPipeline`](crate::transform::TransformPipeline).

mod appendix;
mod attribution_generate;
mod attribution_render;
mod attribution_viewer;
mod authors_normalize;
mod callout;
mod callout_resolve;
mod categories_sidebar;
mod code_block_generate;
mod code_block_render;
mod config;
mod config_markdown;
mod crossref_index;
mod crossref_render;
mod crossref_resolve;
mod date_normalize;
mod equation_label;
mod example_embed;
mod float_ref_target;
mod footer_generate;
mod footer_render;
mod footnotes;
mod link_rewrite;
mod listing_generate;
mod listing_render;
mod mermaid;
mod metadata_normalize;
mod navbar_generate;
mod navbar_render;
pub(crate) mod navigation_active;
mod navigation_enrich;
pub(crate) mod navigation_href;
mod page_nav_generate;
mod page_nav_render;
mod proof;
mod resource_collector;
mod sectionize;
mod shortcode_resolve;
pub(crate) mod sidebar_auto;
mod sidebar_generate;
mod sidebar_render;
mod table_bootstrap_class;
mod theorem;
mod title_banner;
mod title_block;
mod toc_generate;
mod toc_render;
mod website_bootstrap_icons;
mod website_canonical_url;
mod website_favicon;
mod website_title_prefix;

pub use appendix::AppendixStructureTransform;
pub use attribution_generate::AttributionGenerateTransform;
pub use attribution_render::AttributionRenderTransform;
pub use attribution_viewer::AttributionViewerTransform;
pub use authors_normalize::{AuthorsNormalizeTransform, normalize_authors_meta};
pub use callout::CalloutTransform;
pub use callout_resolve::CalloutResolveTransform;
pub use categories_sidebar::CategoriesSidebarTransform;
pub use code_block_generate::{
    CodeBlockDecoration, CodeBlockDecorationKey, CodeBlockGenerateTransform, CopyMode,
    resolve_default_copy_mode,
};
pub use code_block_render::CodeBlockRenderTransform;
pub use config::{
    AppendixStyle, ReferenceLocation, TitleBlockStyle, is_feature_disabled, resolve_website_bool,
};
pub use config_markdown::{ConfigMarkdownTransform, apply_markdown_config_paths};
pub use crossref_index::CrossrefIndexTransform;
pub use crossref_render::CrossrefRenderTransform;
pub use crossref_resolve::CrossrefResolveTransform;
pub use date_normalize::DateNormalizeTransform;
pub use equation_label::EquationLabelTransform;
pub use example_embed::{ExampleEmbedRenderTransform, ExampleEmbedTransform};
pub use float_ref_target::FloatRefTargetSugarTransform;
pub use footer_generate::FooterGenerateTransform;
pub use footer_render::FooterRenderTransform;
pub use footnotes::FootnotesTransform;
pub use link_rewrite::LinkRewriteTransform;
pub use listing_generate::ListingGenerateTransform;
pub use listing_render::ListingRenderTransform;
pub use mermaid::{MERMAID_VERSION, MermaidRenderTransform};
pub use metadata_normalize::MetadataNormalizeTransform;
pub(crate) use metadata_normalize::inlines_to_plain_text;
pub use navbar_generate::NavbarGenerateTransform;
pub use navbar_render::NavbarRenderTransform;
pub use page_nav_generate::PageNavGenerateTransform;
pub use page_nav_render::PageNavRenderTransform;
pub use proof::ProofSugarTransform;
pub use resource_collector::{ResourceCollectorTransform, collect_referenced_asset_urls};
pub use sectionize::SectionizeTransform;
pub use shortcode_resolve::{ShortcodeResolveTransform, extract_shortcode_paths};
pub use sidebar_generate::SidebarGenerateTransform;
pub use sidebar_render::SidebarRenderTransform;
pub use table_bootstrap_class::TableBootstrapClassTransform;
pub use theorem::TheoremSugarTransform;
pub use title_banner::TitleBannerTransform;
pub use title_block::TitleBlockTransform;
pub use toc_generate::TocGenerateTransform;
pub use toc_render::TocRenderTransform;
pub use website_bootstrap_icons::WebsiteBootstrapIconsTransform;
pub use website_canonical_url::WebsiteCanonicalUrlTransform;
pub use website_favicon::WebsiteFaviconTransform;
pub use website_title_prefix::WebsiteTitlePrefixTransform;

/// Append an HTML literal to the canonical `rendered.includes.<slot>`
/// list (`slot` is `"header"`, `"before-body"`, or `"after-body"`) —
/// the location populated by
/// [`IncludeResolveStage`](crate::stage::IncludeResolveStage) and read
/// by the HTML template (`$header-includes$` / `$include-after$`) and
/// the reveal scaffold
/// ([`render_revealjs_document`](crate::revealjs::render_revealjs_document)).
///
/// `sentinel` must be a substring embedded in `payload` (typically an
/// HTML comment); if any existing entry already contains it, the
/// append is skipped, making transform re-runs idempotent. Shared by
/// [`AttributionViewerTransform`] and [`MermaidRenderTransform`].
pub(crate) fn append_with_sentinel(
    meta: &mut quarto_pandoc_types::ConfigValue,
    slot: &str,
    sentinel: &str,
    payload: String,
) {
    use quarto_pandoc_types::ConfigValue;
    use quarto_pandoc_types::config_value::ConfigValueKind;

    if !matches!(&meta.value, ConfigValueKind::Map(_)) {
        return;
    }
    let source_info = meta.source_info.clone();

    if !meta.contains_path(&["rendered", "includes", slot]) {
        meta.insert_path(
            &["rendered", "includes", slot],
            ConfigValue::new_array(vec![], source_info.clone()),
        );
    }

    let Some(target) = meta.get_path_mut(&["rendered", "includes", slot]) else {
        return;
    };
    let ConfigValueKind::Array(items) = &mut target.value else {
        return;
    };
    if items
        .iter()
        .any(|item| item.as_str().is_some_and(|s| s.contains(sentinel)))
    {
        return;
    }
    items.push(ConfigValue::new_string(payload, source_info));
}

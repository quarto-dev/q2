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
mod callout;
mod callout_resolve;
mod config;
mod crossref_index;
mod crossref_render;
mod crossref_resolve;
mod equation_label;
mod float_ref_target;
mod footer_generate;
mod footer_render;
mod footnotes;
mod link_rewrite;
mod metadata_normalize;
mod navbar_generate;
mod navbar_render;
mod navigation_active;
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
mod theorem;
mod title_block;
mod toc_generate;
mod toc_render;
mod website_canonical_url;
mod website_favicon;
mod website_title_prefix;

pub use appendix::AppendixStructureTransform;
pub use callout::CalloutTransform;
pub use callout_resolve::CalloutResolveTransform;
pub use config::{AppendixStyle, ReferenceLocation, is_feature_disabled};
pub use crossref_index::CrossrefIndexTransform;
pub use crossref_render::CrossrefRenderTransform;
pub use crossref_resolve::CrossrefResolveTransform;
pub use equation_label::EquationLabelTransform;
pub use float_ref_target::FloatRefTargetSugarTransform;
pub use footer_generate::FooterGenerateTransform;
pub use footer_render::FooterRenderTransform;
pub use footnotes::FootnotesTransform;
pub use link_rewrite::LinkRewriteTransform;
pub use metadata_normalize::MetadataNormalizeTransform;
pub use navbar_generate::NavbarGenerateTransform;
pub use navbar_render::NavbarRenderTransform;
pub use page_nav_generate::PageNavGenerateTransform;
pub use page_nav_render::PageNavRenderTransform;
pub use proof::ProofSugarTransform;
pub use resource_collector::ResourceCollectorTransform;
pub use sectionize::SectionizeTransform;
pub use shortcode_resolve::{ShortcodeResolveTransform, extract_shortcode_paths};
pub use sidebar_generate::SidebarGenerateTransform;
pub use sidebar_render::SidebarRenderTransform;
pub use theorem::TheoremSugarTransform;
pub use title_block::TitleBlockTransform;
pub use toc_generate::TocGenerateTransform;
pub use toc_render::TocRenderTransform;
pub use website_canonical_url::WebsiteCanonicalUrlTransform;
pub use website_favicon::WebsiteFaviconTransform;
pub use website_title_prefix::WebsiteTitlePrefixTransform;

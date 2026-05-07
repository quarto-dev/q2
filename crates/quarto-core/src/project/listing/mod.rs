/*
 * project/listing/mod.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Listing data model + per-host-page configuration.
//!
//! See `claude-notes/plans/2026-05-06-listings-L2-data-model.md`
//! for the design contract this module implements, and
//! `claude-notes/plans/2026-05-06-listings-L3-resolve-transform.md`
//! for the consumer-side render pipeline (the generate / render
//! transforms in `crate::transforms::listing_*`).
//!
//! This module owns:
//!
//! - **`config`** — the [`Listing`] struct and supporting enums,
//!   plus the `ConfigValue → Vec<Listing>` parser used by the
//!   generate transform.
//! - **`item`** — the [`ListingItem`] struct and the hydration
//!   from a [`crate::document_profile::DocumentProfile`] +
//!   project metadata that produces one item.
//! - **`filter`** — `include` / `exclude` predicate evaluation.
//!   Curated → `extra` fallback per L3 D12.
//! - **`sort`** — multi-key stable sort over hydrated items.
//! - **`placeholders`** — Q1-verbatim regex tokens shared with
//!   the (future) L7 post-render upgrade.
//!
//! The two AST transforms that consume these types live in
//! `crate::transforms::listing_generate` and
//! `crate::transforms::listing_render`.

pub mod binding;
pub mod config;
pub mod filter;
pub mod helpers;
pub mod item;
pub mod placeholders;
pub mod sort;
pub mod templates;

// Native-only L7 post-render upgrade. Excluded from the WASM build
// because it depends on `scraper` (HTML reader for sibling rendered
// outputs) and on file-IO patterns that have no analogue in the
// hub-client preview environment. See plan §"scraper dep gating".
#[cfg(not(target_arch = "wasm32"))]
pub mod post_render_upgrade;

pub use config::{
    ColumnType, FeedType, GridItemAlign, ImageAlign, Listing, ListingCategoriesMode,
    ListingContents, ListingFeedOptions, ListingFilter, ListingSort, ListingType, SortDirection,
    parse_listings,
};
pub use item::{ListingItem, hydrate_item};

/// One fully-resolved listing — its config plus the hydrated item
/// set in final sort order. Produced by `ListingGenerateTransform`
/// and stored on `RenderContext::resolved_listings`; consumed by
/// `ListingRenderTransform` (which builds the per-item template
/// binding and applies the doctemplate).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedListing {
    pub listing: Listing,
    pub items: Vec<ListingItem>,
}

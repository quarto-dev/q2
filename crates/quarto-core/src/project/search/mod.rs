/*
 * project/search/mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Website search (bd-eip0naf4): configuration, per-page capture, and
 * the Pagefind-backed index writer.
 */

//! Client-side full-text search for website projects.
//!
//! The user-facing surface is a port of Quarto 1's `website.search`
//! (navbar / sidebar placement, overlay or textbox, grouped results
//! with `collapse-after`, crumbs, keyboard shortcut, copy button,
//! `?q=` highlighting on the target page). The engine is not a port:
//! instead of one monolithic `search.json` scanned by Fuse.js in the
//! browser, `q2 render` builds a chunked [Pagefind] index with the
//! `pagefind` crate as a library and the browser fetches only the
//! chunks a query touches.
//!
//! Three halves, mirroring llms.txt:
//!
//! - **config** ([`config`]): `resolve_website_search` turns the
//!   `website.search` value (bool or map) into [`config::SearchOptions`].
//! - **capture**: a pipeline stage between `RenderHtmlBodyStage` and
//!   `ApplyTemplateStage` stores each page's body HTML plus title,
//!   language and crumbs as a path-less Project-scoped artifact.
//! - **emit** ([`index_writer`]): `WebsiteProjectType::post_resources`
//!   feeds the captures to Pagefind and writes the bundle under
//!   `site_libs/quarto-search/pagefind/`, a `manifest.json` beside it,
//!   and, when Algolia is configured or `search-json: true` asks for
//!   it, a Quarto 1-shaped `search.json` at the site root.
//!
//! The index is a whole-site, post-render artifact, so the emit half
//! is native-only; the WASM hub-client preview renders one page at a
//! time and shows no search box. Design and decisions:
//! `claude-notes/plans/2026-10-14-website-search.md`.
//!
//! [Pagefind]: https://pagefind.app

#[cfg(not(target_arch = "wasm32"))]
pub mod capture;
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod index_writer;
#[cfg(not(target_arch = "wasm32"))]
pub mod records;
pub mod ui;
#[cfg(not(target_arch = "wasm32"))]
pub mod writer;

pub use config::{
    AlgoliaOptions, FederatedSite, ItemContext, SearchLocation, SearchOptions, SearchType,
    has_navbar, page_search_enabled, resolve_website_search,
};

/// Directory under the project lib dir (`site_libs/`) that holds the
/// search assets: `pagefind/` (the index bundle and its runtime) and
/// `manifest.json`.
pub const SEARCH_LIB_SUBDIR: &str = "quarto-search";

/// The Pagefind bundle directory inside [`SEARCH_LIB_SUBDIR`].
pub const PAGEFIND_BUNDLE_SUBDIR: &str = "pagefind";

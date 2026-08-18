/*
 * project/listing/feed/mod.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! L9 RSS feed generation (`bd-o90m`).
//!
//! See `claude-notes/plans/2026-05-08-listings-L9-rss-feeds.md` for
//! the design. This submodule owns:
//!
//! - **`binding`** — typed `FeedChannel` / `FeedItem` shapes plus
//!   server-side XML escaping and per-item image metadata via the
//!   `imagesize` crate. Native-only (`imagesize` is target-gated in
//!   `quarto-core`'s `Cargo.toml`).
//! - **`stage`** — the `ListingFeedStageTransform` that emits one
//!   `*.feed-{full|partial|metadata}-staged` file per
//!   feed-configured listing during Pass-2. Per-category sub-feeds
//!   produce additional `*-<category>.feed-…-staged` files. Native-
//!   only.
//! - **`complete`** — the `complete_staged_feeds` post-render step
//!   (called from `WebsiteProjectType::post_render` after L7's
//!   substitution) that substitutes the description-element
//!   placeholders against engine-rendered sibling HTML and
//!   finalizes each staged feed into a real `.xml`. Native-only.
//! - **`reader_ext`** — the listings-RSS subset of Q1's
//!   `readRenderedContents`: `extract_first_para_html` (for
//!   `partial` feeds, with HTML preservation and anchor unwrap)
//!   and `extract_full_contents` (for `full` feeds, with
//!   urls-to-absolute, title-block-header removal, and local-
//!   anchor strip). Native-only — depends on `scraper`.
//! - **`link_inject`** — the `ListingFeedLinkTransform` that
//!   injects `<link rel="alternate" type="application/rss+xml">`
//!   into the host page's head metadata. **Runs on both native
//!   and WASM** so the rendered HTML is byte-for-byte identical
//!   between the CLI and the hub-client preview, even though the
//!   linked file is only written by `quarto render`.
//!
//! Module-gate granularity: most files here sit under
//! `#[cfg(not(target_arch = "wasm32"))]` because they pull
//! native-only deps (`imagesize`, `scraper`) or perform file I/O.
//! `link_inject` is the exception: it does no I/O and is registered
//! in the shared `build_html_pipeline_stages_with_apply_config`, which
//! serves native and WASM alike.

#[cfg(not(target_arch = "wasm32"))]
pub mod binding;

#[cfg(not(target_arch = "wasm32"))]
pub mod stage;

#[cfg(not(target_arch = "wasm32"))]
pub mod reader_ext;

#[cfg(not(target_arch = "wasm32"))]
pub mod complete;

#[cfg(not(target_arch = "wasm32"))]
pub use complete::complete_staged_feeds;

// Outside the cfg gate: link-inject does no I/O, has no native-
// only deps, and runs on both native and WASM pipelines. See the
// header comment above and `feed/link_inject.rs`'s file-level
// docs.
pub mod link_inject;

pub use link_inject::ListingFeedLinkTransform;
#[cfg(not(target_arch = "wasm32"))]
pub use stage::ListingFeedStageTransform;

/*
 * project/listing/post_render_upgrade.rs
 * Copyright (c) 2026 Posit, PBC
 */

#![cfg(not(target_arch = "wasm32"))]

//! L7 post-render upgrade for listing previews.
//!
//! ──────────────────────────────────────────────────────────────────
//! **BRACKETING RULES** (from the listings epic plan §L7).
//!
//! This module reads sibling rendered HTML to upgrade listing
//! previews. **This is the only place in Q2 that does this.** Do not
//! add more features here; if you find yourself wanting to read
//! sibling rendered HTML for a different reason, that is a signal to
//! redesign rather than to extend this module.
//!
//! - **Single home.** All L7 code lives in this file (or a sibling
//!   `post_render_upgrade/<name>.rs` module under the same parent
//!   if this file outgrows ~600 LOC). One named function in
//!   `WebsiteProjectType::post_render` reaches into rendered output
//!   files for sibling-content reasons; nothing else.
//! - **CLI-only by construction.** The whole module is gated to
//!   native targets via `#![cfg(not(target_arch = "wasm32"))]`. The
//!   hub-client preview and any future `quarto preview` *do not*
//!   invoke this step. Listings in those environments display the
//!   L1 fallbacks (per the L1 safeguard contract).
//! - **No cross-feature reuse.** If a future feature (search
//!   indexing, social meta, etc.) needs sibling rendered content, it
//!   gets its own named step in `post_render`, not a hook into this
//!   module's machinery.
//! - **Mandatory L1 fallback contract.** Every listing item must be
//!   presentable without L7 running. L7 is an *upgrade*, not a
//!   *requirement*. Reviewers of L1 / L3 / L7 must verify this
//!   property holds: removing L7 from `post_render` produces correct
//!   (if less pretty) listings.
//!
//! See `claude-notes/plans/2026-05-05-listings-epic.md` §L7 for the
//! full rationale, and
//! `claude-notes/plans/2026-05-07-listings-L7-postrender-upgrade.md`
//! for this phase's design.
//! ──────────────────────────────────────────────────────────────────

// Reader: listings-only subset of Q1's `readRenderedContents`. Math
// handling, syntax-highlight class maps, urls-to-absolute, anchor
// stripping are RSS-only (L9's surface). The `ReaderOptions` struct
// is structured so L9 can extend it without breaking L7's call
// sites.
mod reader;

// Substitute: walks every output, regex-finds the L3 envelopes,
// reads referenced sibling outputs (via the reader, with a
// per-call cache), and rewrites each host file in place.
mod substitute;

pub(crate) use substitute::substitute_listing_placeholders;

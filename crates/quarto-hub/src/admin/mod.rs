//! Sync-server maintainer tools (`hub admin …`, bd-eiku4ymo).
//!
//! Design: `claude-notes/plans/2026-07-24-capture-meta-and-hub-admin-tools.md`.
//!
//! The safety model is a pipeline of independent gates — a mistake
//! must survive all of them before bytes are gone:
//!
//! 1. [`scan`] is read-only and evidence-based; it emits a versioned
//!    manifest of orphaned engine-capture docs (allowlist: nothing
//!    else is ever collectible) plus a full inventory.
//! 2. `collect` consumes only a manifest, re-verifies every candidate
//!    against current storage, and **quarantines** (renames into
//!    `<data_dir>/trash/<batch>/`) — it never unlinks.
//! 3. `restore` moves a batch back, hash-verified.
//! 4. `purge` is the only unlink: whole trash batches, past a
//!    retention window, dry-run by default.

pub mod classify;
pub mod collect;
pub mod manifest;
pub mod scan;

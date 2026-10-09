---
title: 'P2 — Pin the `pdf-extract` fork, verify no regression'
date: 2026-09-27
---

**Date:** 2026-09-27
**Epic:** [`2026-09-27-typst-smoke-all-epic.md`](2026-09-27-typst-smoke-all-epic.md) —
read "Decided" items 1 and 3 first: the fork is a permanent, accepted dependency, and
the research session that produced it (`typst-text-positions`) no longer exists — this
phase captures everything it handed off.
**Depends on:** nothing. Can run in parallel with P1.
**Worktree:** `workspace-3` (bootstrap phase — see epic's "Parallel development
plan"). No dependency on P1; do them in either order, sequentially, in this one
worktree.

## Worktree & git workflow

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
# One-time, only if it doesn't exist yet:
git rev-parse --verify feature/typst-testing 2>/dev/null || \
  git checkout -b feature/typst-testing explore/typst-smoke-all-epic
git checkout -B typst-testing/p2-pdf-extract-dependency feature/typst-testing
```

Implement the checklist below, gating on `cargo clippy -p quarto-core --all-targets
-- -D warnings` + `cargo nextest run -p quarto-core`. When done, flip this doc's
checklist to `[x]` and `## Status` to `Complete.`, commit, then:

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
git rebase feature/typst-testing               # pick up anything P1 merged meanwhile
git checkout feature/typst-testing
git merge --ff-only typst-testing/p2-pdf-extract-dependency   # retry from rebase if not a fast-forward
cargo nextest run --workspace                  # phase-boundary gate
```

## Background

`crates/quarto-core/Cargo.toml:197` currently pins `pdf-extract = "0.7"` from
crates.io (comment on `:196`: "needs no external binary" — the reason it was chosen
over `pdftotext`/pdfium/mupdf in the first place). A parallel research thread forked
it to surface tagged-PDF marked-content (MCID) data, which is a prerequisite for P3's
`/StructTreeRoot` walk:

- **Fork:** `https://github.com/gordonwoodhull/pdf-extract`, branch
  `mcid-marked-content`, commit **`f68ca43f27b1e23d92072cc4383178a33ba25457`** (verified
  against `origin`, not just local). Pin by this SHA, not the branch name (branches are
  mutable/force-pushable).
- **Dependency line:**
  ```toml
  pdf-extract = { git = "https://github.com/gordonwoodhull/pdf-extract", rev = "f68ca43f27b1e23d92072cc4383178a33ba25457" }
  ```
- **API added** (`src/lib.rs`, all `pub`, non-breaking — default `Ok(())` bodies on
  the trait methods):
  ```rust
  // On the OutputDev trait:
  fn begin_marked_content(&mut self, tag: Option<&str>, properties: Option<&Dictionary>) -> Result<(), OutputError>;
  fn end_marked_content(&mut self) -> Result<(), OutputError>;

  // Free functions:
  pub fn mcid(properties: Option<&Dictionary>) -> Option<i64>;
  pub fn is_artifact(tag: Option<&str>) -> bool;
  ```
  `Dictionary`/`Object`/`Document` are `lopdf` types already re-exported by
  `pdf-extract` (`pub use lopdf::*`) — no separate `lopdf` dependency needed. Every
  `BMC`/`BDC` fires `begin_marked_content` and every matching `EMC` fires
  `end_marked_content` unconditionally (even when tag/properties fail to resolve —
  `None` in that case), so nesting depth as seen by a consumer always exactly matches
  the content stream's own bracket nesting.
- **Tests:** `src/lib.rs`, `#[cfg(test)] mod marked_content_tests` (starts line 2474):
  `inline_mcid_surfaces`, `named_properties_mcid_resolves`, `bare_bmc_has_no_properties`,
  `artifact_tag_detected`, `nested_marked_content_stays_balanced`. Each builds a
  synthetic one-page PDF in-memory via `lopdf` (no fixture files) — good templates for
  P3's own synthetic-PDF struct-tree tests.
- **Version-jump risk, empirically characterized during plan review:** the patch is
  against `pdf-extract` master (0.12.x lineage); q2 pins 0.7. Adopting the fork is an
  implicit 0.7→0.12+ upgrade spanning panic-hardening work and a font-cache-across-pages
  fix, not just one added feature — so drop-in compatibility isn't something to
  assume. A spike built both versions as standalone binaries (crates.io `"0.7"` vs.
  the pinned git rev) and ran `extract_text()` on 5 real Typst-produced PDFs,
  including the densest, most representative one — the real multi-chapter
  `orange-book` book PDF (`external-sources/quarto-cli/tests/docs/smoke-all/typst/orange-book/_book/Test-Typst-Book.pdf`,
  23,057 bytes of extracted text). **Result: byte-for-byte identical output on all
  5**, no panics, no stderr output either version. This substantially de-risks (but,
  being 5 fixed PDFs rather than the actual 6 test fixtures\' generated PDFs, doesn't
  fully replace) this phase's own `cargo nextest run -p quarto-core` regression gate
  below — treat that gate as still required, now with high prior confidence it'll
  pass rather than a genuine unknown.

## Checklist

- [x] Updated `crates/quarto-core/Cargo.toml`\'s `[dev-dependencies]` entry to the pinned git revision; it remains dev-only.
- [x] `cargo build -p quarto-core` — passed against the pinned fork and current usage. The regression suite covers the six files using `pdf_extract` (ten call sites): `book_theorem_crossref.rs`, `book_citations.rs`, `book_appendix_letter_parity.rs`, `book_numbering_torture.rs`, `book_part_appendix.rs`, and `book_numbering_pipeline.rs`. This set differs from P10's coexistence set.
- [x] `cargo nextest run -p quarto-core` — 5,282 passed, 32 skipped, including all six files above; no text-extraction drift.
- [x] `cargo clippy -p quarto-core --all-targets -- -D warnings` — passed.
- [x] Documented the accepted personal-fork provenance and commit-SHA pin at the `Cargo.toml` entry (Decision 1).

**WASM / cross-platform compliance**: no action needed. `pdf-extract` lives under
`[dev-dependencies]`, so it's excluded from every consumer's normal build, wasm32
included, regardless of this swap. `.claude/rules/cross-platform.md`: the version
bump changes no platform-specific code paths.

## Status

Complete.

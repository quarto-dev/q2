---
title: 'bd-3zp3z4jx — link URL corrupted on write-back'
date: 2026-06-23
---

**Date:** 2026-06-23
**Branch:** `braid/bd-3zp3z4jx-link-url-corrupted-write` (off `origin/main`)

## Symptom

Editing a paragraph so it contains a link whose URL differs from the original
(adding a new link, or changing an existing link's URL) persisted the **wrong**
URL: a new link inherited an *adjacent* link's URL, and a changed URL silently
reverted to the old one.

Discovered while testing the rich-text editor (bd-sjb4pzx8), but the bug is in
the **shared write-back path** (`apply_node_edit` → reconcile → incremental
writer), so it affects the monospaced textarea editor's link edits too.

## Root cause

The incremental writer's **inline splice** (`assemble_recursed_container` in
`crates/pampa/src/writers/incremental.rs`) preserves a container inline's
*delimiters* verbatim from the original source while re-writing only its
children. For a `Link`, the closing delimiter is `](url "title")` — i.e. the URL
is part of the verbatim-copied delimiter, **not** a recursable child.

The reconciler (`compute_inline_alignments` in
`crates/quarto-ast-reconcile/src/compute.rs`) decided two container inlines
"correspond" (→ `RecurseIntoContainer`) using **only the type discriminant**.
So any new `Link` matched any old `Link` regardless of target, and the splice
then copied the old link's `](url)` over the new one.

## Fix

In the reconciler's type-match step, require a container's **non-child identity**
to be unchanged before treating two containers as the same (and recursing):

- `Link` / `Image`: `target` (url, title) **and** `attr` equal
- `Span`: `attr` equal
- (`Custom`: `type_name` equal — pre-existing)

When that identity differs, the new inline falls through to `UseAfter`, which
re-serializes it via the qmd writer (which emits the URL straight from the AST —
`link.target.0`), producing the correct output. Single-line, value-only
comparison; no type or API changes.

## Tests (TDD — failing first)

`crates/pampa/tests/integration/node_edit_tests.rs`:

- `apply_node_edit_preserves_distinct_link_urls` — adding a 2nd link keeps both
  distinct URLs.
- `apply_node_edit_changes_existing_link_url` — changing a link's URL persists.

Both failed before the fix (new link got the old URL), pass after.

## Verification

- `quarto-ast-reconcile` + `pampa`: 4248 pass.
- Full workspace `cargo nextest run --workspace`: **10337 pass**, 0 regressions.
- `cargo xtask verify --skip-hub-build`: Rust build + clippy (`-D warnings`) +
  fmt clean. (ts-packages/hub legs need `npm install`; not affected by this
  Rust-only change — CI covers them.)

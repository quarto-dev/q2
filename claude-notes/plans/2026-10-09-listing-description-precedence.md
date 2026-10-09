---
title: 'Listing ignores an explicit description: and uses the auto-derived first paragraph (bd-listing-description-precedence-x4bh6w3m)'
date: 2026-10-09
description: 'Default and grid listings replace authored descriptions with the derived first paragraph because the L7 envelope is unconditional; make derivation a fallback, as in Quarto 1, and design it together with the missing-description sibling.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-listing-description-precedence-x4bh6w3m
---

**Branch:** `braid/bd-listing-description-precedence-x4bh6w3m-listing-ignores-explicit-description` (main checkout, topic branch, based on `main` @ `ea72d68aa`)
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The bug reproduces at HEAD, and the cause is the one
named in the strand's 2026-10-09 comment: the L7 envelope is unconditional.
One design decision blocks implementation. This strand and
bd-listing-default-no-derived-desc-m0wrr8ty are two halves of a single
precedence rule, and fixing either alone either breaks the other or leaves a
latent regression. The fix needs a way to tell an authored description from
a derived one.

## Issue context

The strand is a P2 bug (labels `listings`, `parity`), filed 2026-08-11 from
the Connect docs port (origin `br-zx6111f9`). Its original diagnosis was L1:
`ListingItemInfoStage`'s `fill_string_if_absent` checks only
`listing-item.description`, so a top-level `description:` doesn't stop the
autofill.

A 2026-10-09 comment (after #810/#812) moves the cause to L7. The item data
now carries the explicit description, and a table listing shows it. Default
and grid listings still show the body paragraph, because:

- `binding.rs` (~441) sets `placeholders = item.origin == ItemOrigin::Document`
  and emits `description-placeholder-begin/-end` for every document item.
- `post_render_upgrade/substitute.rs` (`substitute_descriptions`, ~143)
  replaces the envelope's contents with the sibling page's first paragraph
  whenever that paragraph is non-empty.

Quarto 1 (`website-listing-read.ts:1127`) emits the placeholder only as a
fallback:
`documentMeta?.description || documentMeta?.abstract || descriptionPlaceholder(...)`.

The bug surfaced in bd-fvcip3t5. Switching the claude-notes plans listing to
`type: grid` made all ~1050 cards show a body excerpt instead of the curated
`description:`.

## Dependency graph

The strand has no formal edges. The informal links:

- **Surfaced by** bd-fvcip3t5 (claude-notes plans listing, epic
  bd-uk8zgkha), which is in progress. A grid layout for the plans page is
  blocked on this strand.
- **Sibling** bd-listing-default-no-derived-desc-m0wrr8ty (P3, in progress,
  plan `2026-08-20-listing-default-derived-description.md`). It covers the
  inverse case: a page *without* `description:` gets no description, because
  Pass-1 (`orchestrator.rs` `pass1_profile_single_file_live`, ~2633) omits
  `ListingItemInfoStage`, and `$if(description)$` suppresses the envelope.
  This is still true at HEAD; I re-checked the stage list. **The two strands
  should be linked `related` at least, and are a candidate to merge** (see Q1).

## What the code looks like today

The repro lives in `claude-notes/plans/listing-description-precedence-investigation/`
(see its README for commands and the full table). It has four posts (explicit
`description`, nothing, `listing-item.description`, `abstract`) and three
listings (default, grid, table). It was run with q2 and with Quarto 1:

| listing | explicit | none | listing-item | abstract |
|---------|----------|------|--------------|----------|
| q2 default/grid | BODY ✗ | — ✗ | BODY ✗ | — ✗ |
| q2 table | EXPLICIT ✓ | — ✗ | LISTING-ITEM ✓ | — ✗ |
| Q1 | EXPLICIT | BODY | BODY (no `listing-item` in Q1) | ABSTRACT |

The relevant code, layer by layer:

- **L1** (`stage/stages/listing_item_info.rs` ~100–126). `autofill_listing_item`
  still checks only `listing-item.<key>`. The stage runs in the full
  transform pipeline (`pipeline.rs:332`) but not in Pass-1, so listing
  profiles never see its output today. That is why the original L1 symptom
  is currently masked.
- **Hydration** (`project/listing/item.rs:216`). The chain is
  `li.description.or(profile.description)`, so `listing-item.description`
  shadows the top-level key. **Latent regression:** if the sibling strand's
  Phase 1 adds `ListingItemInfoStage` to Pass-1, the autofilled
  `listing-item.description` will beat an explicit top-level `description:`
  in *every* listing type, table included. The original L1 bug comes back.
- **L3 binding** (`project/listing/binding.rs` ~438–462). The envelope
  decision is per-origin, not per-provenance.
- **Templates.** `item-default.template:47` and `item-grid.template:53` put
  the envelope inside `$if(description)$`. The table row is prebuilt in Rust
  (`$table-row$`) with no envelope, so tables never derive.
- **L7** (`post_render_upgrade/substitute.rs`). It replaces unconditionally,
  and the "keep L1 inner" path only fires when the sibling paragraph is
  missing or empty.
- **`abstract`** isn't on `DocumentProfile` and isn't in the hydrate chain.

## Proposed phases (draft)

- **Phase 0 — Tests first.** Add an end-to-end orchestrator test over the
  repro shape that asserts the Q1 column, with LISTING-ITEM-C for the
  `listing-item` case, across default, grid, and table. It must go through
  real Pass-1 profiles, not hand-built ones. Add unit tests for the binding's
  envelope decision.
- **Phase 1 — Provenance.** Give `ListingItem` (or the profile) a way to
  tell "author supplied a description" from "derived / absent". Q2 covers
  the options.
- **Phase 2 — Fallback-only envelope.** Emit the envelope only when there is
  no authored description. In the templates, move the envelope out of
  `$if(description)$` so items with no description still get one (this
  overlaps the sibling).
- **Phase 3 — L1 precedence.** Make `fill_string_if_absent` for
  `description` also respect top-level `description` (and `abstract`, per
  Q3), so adding L1 to Pass-1 can't reintroduce the bug.
- **Phase 4 (optional, per Q3/Q4)** — `abstract` fallback; table
  derivation.
- **Phase 5 — Verify on the real site.** Re-render the claude-notes plans
  page with `type: grid` (bd-fvcip3t5) and the Connect-docs repro, and
  update listing docs if behavior changes.

## Open design questions for the user

1. **Merge with the sibling?** Should this strand absorb
   bd-listing-default-no-derived-desc-m0wrr8ty, so one plan owns "derive
   only when nothing is authored"? Or should we keep both and land this one
   first, with the sibling's Pass-1 change gated on Phase 3 here?
2. **How to mark provenance.** Choose one:
   (a) a `description_is_derived: bool` (or enum) on `ListingItem`, set in
   `hydrate_item`;
   (b) decide from `SourceInfo` (`By::programmatic_config()` marks L1 autofill);
   (c) stop L1 from autofilling `description` into `listing-item` at all,
   and let L7 be the only derivation path. This matches Q1, which never
   precomputes a description.
   I lean toward (c) if the L1 value has no other consumer (feeds? search?
   to check). Otherwise (a).
3. **`abstract` fallback.** Should we add `description → abstract → derived`
   for Q1 parity now, or file a separate strand?
4. **Table listings.** Q1 tables show derived descriptions; q2 tables never
   do. Is that in scope here, or a separate parity strand?
5. **`listing-item.description` vs top-level `description`.** Keep the
   current rule that `listing-item` wins when both are set? (It's a q2-only
   key; the integration test at `document_profile_pipeline.rs` ~602 assumes
   it wins.)

## Risks / tradeoffs (draft)

- **Ordering hazard with the sibling.** Landing the sibling's Pass-1 change
  alone makes authored descriptions lose in tables too. Either land them
  together or land Phase 3 first.
- **Truncation semantics.** Authored descriptions aren't truncated today in
  L7, because L7 replaces them. Under Q1, authored descriptions aren't
  truncated by `max-description-length` either. Check that this holds once
  they bypass L7.
- **Feeds and search.** Option (c) in Q2 changes what non-HTML consumers of
  `listing_item.description` see. Audit `listing/feed` before choosing it.

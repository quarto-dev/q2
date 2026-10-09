---
title: 'Listing ignores an explicit description: and uses the auto-derived first paragraph (bd-listing-description-precedence-x4bh6w3m)'
date: 2026-10-09
description: 'Listings replace authored descriptions with a derived first paragraph and leave undescribed pages empty; give items a description source so derivation is a fallback (listing-item → description → abstract → derived), in every listing type.'
status: draft  # Design agreed 2026-10-09 (all questions answered); awaiting the go-ahead to implement
braid:
  strand: bd-listing-description-precedence-x4bh6w3m
  also: bd-listing-default-no-derived-desc-m0wrr8ty
---

**Branch:** `braid/bd-listing-description-precedence-x4bh6w3m-listing-ignores-explicit-description` (main checkout, topic branch, based on `main` @ `ea72d68aa`)
**Also covers** bd-listing-default-no-derived-desc-m0wrr8ty (merged in 2026-10-09; its investigation is `2026-08-20-listing-default-derived-description.md`).
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The bug reproduces at HEAD, and the cause is the one
named in the strand's 2026-10-09 comment: the L7 envelope is unconditional.
This strand and bd-listing-default-no-derived-desc-m0wrr8ty are two halves of
one precedence rule, so they are now planned together. The provenance
question that blocked the design is settled (see Decisions), and so are the
follow-ups on the shared head pipeline and `image`.

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

## Decisions (2026-10-09)

1. **One plan for both strands.** This plan also owns
   bd-listing-default-no-derived-desc-m0wrr8ty (no description for pages
   without one).
2. **Provenance is explicit: option (a).** Items carry where their
   description came from. The derived text is kept even when an authored
   one wins, because a prospective user wants search, and search should
   be able to use it. Schema and API changes are fine at this stage.
3. **`abstract` fallback is in scope.**
4. **Table listings derive too.** This is in scope.
5. **`listing-item.description` keeps winning** over top-level
   `description:`. It exists so Lua filters can set listing metadata
   programmatically; nobody uses it yet, but it stays.

The resulting precedence, for document-origin items:

```
listing-item.description → description → abstract → derived
```

"Derived" means the first paragraph of the rendered page (L7), with the
pre-engine first paragraph (L1) as the fallback inside the envelope.

## Design

**Profile (`document_profile.rs`).**
- `ListingItemInfo.description` holds only what the author wrote under
  `listing-item:`.
- Add `ListingItemInfo.derived_description: Option<String>`. L1 fills it
  from the first paragraph *always*, not only when nothing is authored, so
  search has it.
- Add a top-level `abstract` to `DocumentProfile`, read with `prose_field`
  like `description`.
- Bump `DOCUMENT_PROFILE_VERSION` (currently 14), so cached profiles are
  discarded. This also answers the sibling's question 4.

**L1 (`listing_item_info.rs`).** Write the derived description to a
reserved key, such as `listing-item.derived-description`, instead of
`listing-item.description`. An authored key is then never touched or
shadowed, and nothing has to decide provenance from `SourceInfo`. (The key
name is an implementation detail; I'll pick one and document it.)

**Hydration (`item.rs`).** Add
`ListingItem.description_source: DescriptionSource`, an enum with the
variants `ListingItem | Description | Abstract | Derived | None`.
`description` follows the chain above. `derived_description` is copied
onto the item as well, for search and future consumers.

**Binding (`binding.rs`).** Emit the description envelope only when
`origin == Document` and `description_source` is `Derived` or `None`.
Expose a `show-description` key that is true when there is a description
*or* an envelope. The default and grid templates switch from
`$if(description)$` to `$if(show-description)$`, so a page with no L1
paragraph still gets an envelope that L7 can fill. Pages whose body starts
with engine output need this; it was the sibling's question 2.

**Table (`table_row`).** The description cell gets the same envelope as
inline raw HTML (`` `<!-- desc-begin… -->`{=html} ``). L7's extraction
returns plain text with no `<p>`, so it is safe inside a cell.

**L7 (`substitute.rs`).** Logic is unchanged: it only ever sees envelopes
for items with no authored description. Q-12-13 ("no preview content;
using static fallback") should stay quiet when there is nothing to fall
back to, either. Q1 silently leaves the text empty in that case.
Otherwise every title-only page would warn now that envelopes are
unconditional.

**Pass-1 (sibling's root cause).** `pass1_profile_single_file_live` gets
`ListingItemInfoStage`, so `derived_description`, image, word count and
reading time reach listing profiles. How to do it is still open (Q-A below).

**Feeds.** Metadata feeds inline `item.description`. That now includes a
derived one when nothing is authored, which matches Q1, where the
placeholder is substituted.

## Phases

- **Phase 0 — Tests first (red).**
  - An orchestrator-level integration test over the repro shape (posts with
    explicit description, no description, `listing-item.description`, and
    abstract, each listed in default, grid and table). It asserts:
    EXPLICIT, BODY, LISTING-ITEM, ABSTRACT in every type. It must drive real
    Pass-1 profiles.
  - Both `listing-item.description` and `description` set →
    `listing-item` wins.
  - A post with top-level `image:` and a different body image → the
    listing shows the `image:` one, in every type. Without `image:` → the
    body image.
  - Pass-1 profile == full-pipeline profile for a fixture that exercises
    includes, `include-in-header`, `lang`, and listing autofill.
  - A page whose body starts with a code cell and has no prose → the
    description comes from L7 (engine output).
  - Unit tests: the `hydrate_item` chain and `description_source`; the
    binding's envelope decision for each source; the table cell envelope;
    L1 writes `derived-description` and leaves `listing-item.description`
    alone.
- **Phase 1 — Profile and L1.** `derived_description`, `derived_image`,
  `abstract`, the version bump, and the L1 key changes.
- **Phase 2 — Hydration.** `DescriptionSource` / `ImageSource` and the
  precedence chains.
- **Phase 3 — Binding and templates.** Conditional description and image
  envelopes,
  `show-description`, table cell envelope, Q-12-13 quieting.
- **Phase 4 — Shared head.** Extract `head_stages()` (SourceConversion →
  Parse → MetadataMerge → LanguageResolve → IncludeExpansion →
  IncludeResolve → ListingItemInfo → DocumentProfile → LinkResolution).
  Use it from both builders. Resolve the cache-key questions above. Add a
  test that the Pass-1 and full-pipeline profiles are equal. Update the
  sibling plan's list of other stage-list builders that must *not* use it.
- **Phase 5 — Real-site check.** Re-render the repro against Quarto 1 (the
  table in the README should match, except LISTING-ITEM-C). Re-render the
  claude-notes plans page with `type: grid` (bd-fvcip3t5), and the
  Connect-docs repros (`listing-description-precedence`,
  `listing-ellipsis-no-matching`). `cargo xtask verify`.
- **Phase 6 — Docs.** Listing docs: description precedence, `abstract`,
  derivation in tables. Update the `ListingItemInfo` doc comments and
  `claude-notes/designs/document-profile-contract.md` if Pass-1's head
  changes.

## Decisions, round 2 (2026-10-09)

- **Q-A → one shared `head_stages()`** used by both Pass-1
  (`pass1_profile_single_file_live`) and the full pipeline
  (`build_html_pipeline_stages_with_options`).
- **Q-B → `IncludeResolveStage` joins Pass-1** as part of this work.
- **Q-C → `image` gets the same treatment as `description`.** Add
  `derived_image` on the profile (L1 writes a reserved key, such as
  `listing-item.derived-image`) and an `ImageSource` on the item. The chain
  is `listing-item.image → image → derived`, and the L7 image envelope is
  emitted only when the image is derived or missing. Q1's rule is
  `image → placeholder`.

### Why the head lists drifted (history check)

Pass-1's head list dates from the websites feature (`dbaa5bbf7`,
2026-05-01). Three stages were later added to the full pipeline only:

| stage | added | Pass-1? | recorded reason |
|---|---|---|---|
| `IncludeResolveStage` | `6421c3333` (2026-05-04, bd-8kp3) | no | none |
| `ListingItemInfoStage` | `ccb220023` (2026-05-08, listings #169) | no | none; the L1 plan never mentions Pass-1 |
| `LanguageResolveStage` | `3bffb3c45` (2026-07-17, i18n) | no, **deliberately** | "profile doesn't carry terms in v1" (`2026-07-17-localization-i18n-design.md:298`) |

`2026-04-16-plan1c-extension-integration.md:1145` (the engine-claims
work) noticed the IncludeResolve/ListingItemInfo gap and **deferred** it.
It called the gap orthogonal and said it "touches Pass-1 profile +
cache-key semantics". That is a caution, not a reason to keep the gap.
Nothing in the history argues against sharing. The i18n omission was a
cost/scope call: terms don't reach the profile, so running the stage in
Pass-1 only costs a language-file read per document. A shared list
includes it. If that cost shows up in timings, we can make an explicit,
commented exception.

The plan1c caution points to real work, which is now part of Phase 4:

- **Pass-1 cache key vs. new inputs.** `cache_key::pass1_key` hashes
  source bytes, metadata files and so on, not filesystem mtimes. L1 fills
  `date_modified` from mtime, so a cached profile would carry a stale
  `date-modified` after a `touch` with no content change. Decide: put mtime
  in the key, leave `date_modified` out of the cached profile and recompute
  it, or accept the staleness and document it. Likewise confirm that
  `IncludeResolveStage`'s file-slot reads (`include-in-header` files) don't
  put file *contents* into the profile in a way the key doesn't cover.
- **Profile equality.** After the change, the Pass-1 profile and the full
  pipeline's profile should be identical for the same input. Add a test
  for that, because it is the invariant that stops future drift.

## Risks / tradeoffs

- **Pass-1 cost and determinism.** L1 reads mtime per document. That makes
  `date_modified` vary across checkouts; check that it doesn't poison the
  profile cache key or snapshots (from the sibling plan).
- **Escaping.** `extract_first_para` returns plain text, and L7 splices it
  into HTML. Check that `<` and `&` in a body paragraph are escaped. A
  derived description now reaches many more items, so a latent escaping
  bug would show up widely.
- **Truncation.** Authored descriptions aren't truncated (Q1 doesn't either).
  Derived ones are truncated by L7 at `max-description-length`. The L1
  fallback text inside the envelope is untruncated today; decide whether to
  truncate it too.
- **Rendered HTML churn.** Unconditional envelopes and table derivation
  change many snapshot outputs. Expect snapshot updates; review them as
  parity improvements rather than accepting them wholesale.

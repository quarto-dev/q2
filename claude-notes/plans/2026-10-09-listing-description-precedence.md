---
title: 'Listing ignores an explicit description: and uses the auto-derived first paragraph (bd-listing-description-precedence-x4bh6w3m)'
date: 2026-10-09
description: 'Listings replace authored descriptions with a derived first paragraph and leave undescribed pages empty; give items a description source so derivation is a fallback (listing-item → description → abstract → derived), in every listing type.'
status: in-progress  # Implementation started 2026-10-09
braid:
  strand: bd-listing-description-precedence-x4bh6w3m
  also: bd-listing-default-no-derived-desc-m0wrr8ty
---

**Branch:** `braid/bd-listing-description-precedence-x4bh6w3m-listing-ignores-explicit-description` (main checkout, topic branch, based on `main` @ `ea72d68aa`)
**Also covers** bd-listing-default-no-derived-desc-m0wrr8ty (merged in 2026-10-09; its investigation is `2026-08-20-listing-default-derived-description.md`).
**Implementation approved 2026-10-09.**

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
reading time reach listing profiles. It uses a shared head-stage list (Q-A, decided below).

**Feeds.** Metadata feeds inline `item.description`. That now includes a
derived one when nothing is authored, which matches Q1, where the
placeholder is substituted.

## Implementation design refinements (2026-10-09, before Phase 0)

These refine the Design section above, based on reading the code.

- **The L1 → profile channel is a typed side-channel, not a meta key.**
  `DocumentAst.derived_listing: DerivedListingValues { description, image }`
  is set by `ListingItemInfoStage` and moved into
  `DocumentProfile.derived_listing` by `DocumentProfileStage`. This is the
  same pattern as `recorded_includes` → `profile.includes`. A reserved
  `listing-item.derived-*` meta key would leak into `listing-item` `extra`
  fields, could be forged from front matter, and would be visible to Lua
  filters as if it were authored. L1 keeps filling `word-count`,
  `reading-time-minutes` and `date-modified` into `meta.listing-item` as
  today. It stops writing `description` and `image` there.
- **One provenance enum for both fields.** `FieldSource { ListingItem,
  Document, Abstract, Record, Derived, Absent }`, with `is_authored()`.
  `ListingItem` gets `description_source` and `image_source`.
- **Gate placeholders by field source, not item origin.** A field gets its
  L7 envelope when the item has a document target (`output_href()`) and its
  source is `Derived` or `Absent`. This replaces
  `origin == ItemOrigin::Document`. Consequence: a `RecordOverDocument`
  whose record doesn't set `description` now derives from the document,
  which is what Q1 does (it spreads the record over the document item,
  whose description is the placeholder). A record that *does* set it stays
  authored. Update the comment in `feed/binding.rs` that contrasts the two
  gates.
- **The envelope wraps the derived value, not an empty slot.** L7 runs
  only in native project renders (`orchestrator.rs` ~566, gated
  `not(wasm32)`). In the hub client, what's inside the envelope is what
  users see. Description: the envelope wraps `$description$` (the L1 text).
  Image: the `$if(image-html)$` branch also gets the envelope, so a derived
  image renders directly and L7 can still upgrade it to the rendered
  page's preview image (which honours `.preview-image`). L7 image fallback
  order becomes: rendered preview → existing inner `<img>` (the L1 image)
  → listing `image-placeholder` default → empty div.
- **Q-12-13** fires only when the envelope held a non-empty L1 fallback.
  An empty envelope with no paragraph in the rendered page is silent (Q1
  parity).
- **Table listings** get the description envelope as inline raw HTML in
  the cell. Table images stay as they are: `image-html`, which now
  includes a derived image.
- **`head_stages()`** is SourceConversion → Parse → MetadataMerge →
  LanguageResolve → IncludeExpansion → IncludeResolve → ListingItemInfo →
  DocumentProfile → LinkResolution. `build_html_pipeline_stages_with_options`
  = `head_stages()` + tail; Pass-1 = `head_stages()`. Every other builder
  already derives from the full list, except `build_analysis_pipeline`
  (LSP: no profile checkpoint, deliberately separate).

## Implementation todo

### Phase 0 — Tests first (red)
- [x] Integration test (orchestrator, real Pass-1): the repro shape — posts
  with explicit description / none / `listing-item.description` / abstract
  / both listing-item+description — in default, grid and table listings.
  Expect EXPLICIT, BODY, LISTING-ITEM, ABSTRACT; listing-item beats
  description.
- [x] Integration test: code-cell-first page with no prose → description
  derived (envelope present even with no L1 text). Simulated with a raw
  HTML `<p>` block: not a Para to L1, but a `<p>` to L7, as engine output is.
- [x] Integration test: top-level `image:` + different body image → `image:`
  shown (default + grid); no `image:` → body image.
- [x] Integration test: Pass-1 profile == full-pipeline profile (fixture
  with include, `include-in-header`, `lang`, listing autofill). It runs the
  full list's head (through `link-resolution`, before `unwrap-profile`)
  through the same `run_pipeline` entry point Pass-1 uses.
- [x] Record red results here.

All in `crates/quarto-core/tests/integration/listing_description_precedence.rs`.
Red results at `ea211d56f` + tests:

| test | result |
|---|---|
| `listing_description_follows_precedence_in_every_listing_type` | FAIL: `default.html` lacks `EXPLICIT-A` |
| `listing_derives_description_present_only_in_rendered_output` | FAIL: `default.html` lacks `RENDERED-ONLY paragraph.` |
| `pass1_profiles_equal_full_pipeline_profiles` | FAIL: `index.qmd`'s Pass-1 profile lacks `listing_item.date_modified` (the L1 mtime) |
| `listing_image_follows_precedence` | **passes today** — Pass-1 has no L1, so `listing-item.image` is never autofilled. It is the guard for the latent image bug once Phase 4 adds L1 to Pass-1. |

Known test that encodes the old behaviour and must change in Phase 3:
`website_post_render.rs::pipeline_website_post_render_substitutes_listing_placeholders`
asserts an explicit `description:` is *replaced* by the body paragraph.

### Phase 1 — Profile + L1
- [x] `DerivedListingValues` type; `DocumentAst.derived_listing`;
  `DocumentProfile.derived_listing`; `DocumentProfile.r#abstract`.
- [x] L1 writes `derived_listing` (always) and stops writing
  `listing-item.description` / `listing-item.image`.
- [x] `DocumentProfileStage` drains the side-channel; `extract` reads
  `abstract`. An abstract written as several paragraphs is flattened
  *without* a Q-12-26 record: it isn't written for the listing.
- [x] Bump `DOCUMENT_PROFILE_VERSION` 14 → 15 with a changelog line.
- [x] Unit tests (L1 never touches authored keys; side-channel drained).
  Updated L1 t01–t08, t14, t16; new t08b; new profile tests for
  `abstract` and `derived_listing` serde; `document_profile_pipeline.rs`
  autofill tests now read `derived_listing`.

Notes: the 45 `DocumentAst { … recorded_includes: Vec::new(), … }`
literals got `derived_listing: Default::default()` mechanically; engine
execution's full destructure threads it through; the book single-file
merge carries it from `crossref_doc`. Between Phase 1 and Phase 2,
derived descriptions reach no listing (hydration doesn't read
`derived_listing` yet). That is expected and is fixed in Phase 2.
`IncludeResolveStage` already records file-slot includes (with content
hashes) into `recorded_includes` → `profile.includes` (relevant to the
Phase 4 cache question).

### Phase 2 — Hydration
- [x] `FieldSource`; `ListingItem.description_source` / `image_source`.
- [x] `hydrate_item` chains; `record_item` / `overlay_record` sources.
- [x] Unit tests for each chain step (`item.rs`: description chain,
  literal derived text, image chain with rebasing, `is_authored`;
  `record.rs`: overlay keeps an unset field's document source, bare
  record sources).

Notes: derived description becomes literal-text inlines
(`split_string_to_inlines`), so markdown-significant characters in a body
paragraph's plain text stay characters. Ten test-only `ListingItem`
literals got `Absent` sources.

### Phase 3 — Binding, templates, L7
- [x] Per-field placeholder gating (`derivable(item, source)`: unauthored
  source + document target); `show-description` key.
- [x] `item-default` / `item-grid` templates: `show-description`; image
  envelope inside the `image-html` branch; `$description$` references
  guarded with `$if(description)$` (an envelope-only item has no
  `description` key, and an unguarded reference is Q-12-10).
- [x] Grid: with a link, the envelope now sits *inside* the link as inline
  raw HTML, so L7's substitution keeps the link (before, the envelope
  wrapped the link and substitution dropped it).
- [x] Table description cell envelope (`description_cell`), and the
  defaulted `description` column survives presence filtering when an
  item is derivable (Q1: every document item carries the placeholder).
- [x] L7: image inner-`<img>` fallback beats the listing default;
  Q-12-13 only with a non-empty fallback.
- [x] Unit tests: binding gating per source, no envelope without a
  document, `show-description`, image gating, table cell, kept table
  column; derived image wrapped in envelope (`listing_render.rs`); L7
  derived-img-over-default and silent empty envelope.

Existing tests updated because they encoded the old behaviour:
- `website_post_render.rs::pipeline_website_post_render_substitutes_listing_placeholders`
  and `…_image_from_sibling_preview`, `listing_pipeline.rs::default_listing_renders_three_posts_in_default_order`
  and `…_derived_ellipsis`: they gave posts an explicit `description:`
  and asserted that L7 *replaced* it. Now: derive when nothing is
  authored; an explicit description stays as written.
- `binding.rs` test helper `item()`: its description is the old L1
  fallback, so its source is `Derived`;
  `record_over_document_keeps_path_but_no_placeholders` marks the
  description as the record's.
- `listing_render.rs::render_omits_image_placeholder_when_l1_image_set`
  → `…_when_image_authored` (an L1 image is now derived and *is*
  wrapped; new test for that).
- `pandoc_request_books.rs` golden: `meta.listing-item.description` (the
  L1 value leaking into every document's pandoc metadata) is gone.
  Re-recorded with `BOOK_REQUEST_GOLDEN=record`; the only diff is those 4
  lines.
- `pandoc_request_projects.rs` book tests passed only because the leaked
  `listing-item.description` MetaString contained "First chapter"
  verbatim (body prose is split into `Str`/`Space` nodes). Switched to
  single-token markers.

### Phase 4 — Shared head
- [x] Extract `build_head_stages()` (`pipeline.rs`); the full HTML builder
  is `build_head_stages()` + tail, and Pass-1
  (`pass1_profile_single_file_live`) runs `build_head_stages()`. It is
  the only hand-maintained head list left in the tree; the WASM, preview,
  pandoc and pause/finishing builders all derive from the full list.
  `build_analysis_pipeline` (LSP) stays separate on purpose: it has no
  profile checkpoint.
- [x] Pass-1 cache key vs. mtime-based `date_modified`: **decided — key
  on the date**. `Pass1KeyInputs::source_modified_date` holds the exact
  `YYYY-MM-DD` that L1 records (same function, `mtime_iso`, now
  `pub(crate)`). A touch on a new day misses the cache; same-day touches
  don't, which matches what the profile records. Presence byte in the
  hash; `PROFILE_KEY_VERSION` 2 → 3; unit test
  `key_changes_on_source_modified_date`.
- [x] `IncludeResolveStage` already records file-slot includes with
  content hashes into `recorded_includes` → `profile.includes`, and
  `profile_cache::load` re-verifies those hashes. Covered, no change.
  Its and `LanguageResolveStage`'s problems are diagnostics, not stage
  errors, and Pass-1 discards diagnostics, so no new Pass-1 failures.
- [x] Pass-1 == full-pipeline profile test is green; plus a structural
  test `full_pipeline_starts_with_the_shared_head`.

Behaviour change surfaced by the suite: the llms.txt companion of a
listing page (`llms.rs`) now shows the derived description for items
without `description:`, matching the HTML listing
(`llms_txt.rs::llms_listing_page_companion_synthesizes_item_list` updated).

### Phase 5 — Verification
- [x] All Phase 0 tests green; `cargo xtask verify --skip-hub-build`:
  16054/16054 Rust tests pass. Its `test:wasm` step failed against a
  **stale** WASM bundle (`--skip-hub-build` doesn't rebuild it). After
  `npm run build:wasm`, one more golden needed regenerating:
  `crates/quarto-core/schemas/pandoc-request.golden.json`
  (`Q2_REGENERATE_GOLDEN=1 … golden_file`). Audited: the only change in
  the embedded pandoc input is `meta.listing-item` losing the leaked
  derived `description` / `image` (blocks identical; `job_id` follows).
  Full `test:wasm` then 414/414.
- [ ] Repro vs Quarto 1 (README table); claude-notes plans page with
  `type: grid`; Connect-docs repros.
- [ ] Review snapshot churn item by item. (No insta snapshot changed;
  the two goldens above are the only recorded-output changes.)

### Phase 6 — Docs
- [x] Listing docs: precedence, `abstract`, derivation in tables, image
  precedence (`docs/guides/projects/listings.qmd` § "Where descriptions
  and images come from").
- [x] `listing-templates.qmd` § "Descriptions and the placeholder
  envelope": markers are empty for authored descriptions; built-ins now
  emit the envelope for every derivable item; in-link envelope shape;
  Q-12-13 condition; image envelope.
- [x] `docs/errors/listing/Q-12-13.qmd` and the catalog message: fires
  only when a pre-render paragraph is the fallback.
- [x] `ListingItemInfo` / `DocumentProfile` doc comments (Phase 1);
  `document-profile-contract.md` field rows + v15 changelog entry;
  `ejs-listing-port` skill note on the envelope.

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
- **Truncation.** (Corrected during Phase 3.) Every description the binding
  emits — authored or the L1 fallback — is already truncated at the
  listing's `max-description-length` (`binding.rs`, bd-pcmdb7qg, Q1
  parity). L7 truncates what it derives from the rendered page at the
  same limit. Nothing to change.
- **Rendered HTML churn.** Unconditional envelopes and table derivation
  change many snapshot outputs. Expect snapshot updates; review them as
  parity improvements rather than accepting them wholesale.

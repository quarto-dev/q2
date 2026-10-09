---
title: 'Grid and default listings ignore fields: for everything but categories (bd-p80b9jy9)'
date: 2026-10-09
description: 'The built-in grid and default item templates gate only categories on the effective field set; gate every field block on its `show.*` flag as Quarto 1 does, so fields: [date, title, description] stops drawing image placeholders.'
status: in-progress  # Implementation approved 2026-10-09
braid:
  strand: bd-p80b9jy9
---

**Branch:** `braid/bd-p80b9jy9-listing-fields-gating` (main checkout, topic branch, based on `main` @ `ea72d68aa`)
**Implementation approved 2026-10-09.**

**Pre-flight (2026-10-09):** `cargo xtask verify --skip-hub-build` passed all Rust tests (16029/16029). The ts-packages step failed because `@bjorn3/browser_wasi_shim` and `@myriaddreamin/typst.ts` are missing, which is a stale `node_modules` in this checkout (run `npm install`), not a problem at HEAD.

## Triage verdict

**Ready to design.** The bug reproduces at HEAD exactly as described, the fix
direction in the strand holds up (the `show` map is already there and the
default field sets already contain `image`), and the only real design
decisions are scope (how much more of Q1's template structure to port) and
sequencing against the in-flight description-precedence work, which edits the
same two templates.

## Issue context

P2 bug (labels `listings`, `parity`), filed 2026-10-09 by Carlos Scheidegger.
`item-grid.template` and `item-default.template` consult the per-item `show`
map only for categories (`$if(show.categories)$`). Image, title, subtitle,
description, author, date and reading time render whenever the item has a
value, whatever `fields:` says. The visible case: a grid listing with
`fields: [date, title, description]` draws a `card-img-top` with a
`listing-item-img-placeholder` on every card.

## Dependency graph

- **discovered-from** bd-fvcip3t5 (claude-notes plans listing page, epic
  bd-uk8zgkha, in progress). Trying `type: grid` with
  `fields: [date, title, description]` on `claude-notes/plans/index.md`
  produced 1053 cards each topped by an empty grey placeholder bar. The plans
  page is a table today, so nothing is blocked; grid is just not usable yet.
- No `blocks` or `related` edges. Informally related to
  bd-listing-description-precedence-x4bh6w3m and
  bd-listing-default-no-derived-desc-m0wrr8ty, which another agent is
  implementing now (plan `2026-10-09-listing-description-precedence.md`, on a
  branch in a different checkout). That plan's Phase 3 rewrites the
  description block (`$if(description)$` → `$if(show-description)$`) and the
  `$if(image-html)$` branch (adds an image envelope) **in these same two
  templates**. See Q2.

## What the code looks like today

Repro: `claude-notes/plans/listing-fields-gating-investigation/` (README has
commands and results). Two posts with every field set (one with an image),
and default/grid listings with and without `fields: [date, title, description]`.

- **Templates** (`crates/quarto-core/src/project/listing/templates/`).
  `item-grid.template` and `item-default.template` gate image (both the
  `image-html` and the placeholder branch), title, subtitle, reading-time,
  description, author and date on value presence only. `show.categories` is
  the only `show.*` read, and that has been so since the listings feature
  landed (`ccb220023`).
- **Binding** (`binding.rs` ~503). Every item gets `show.<field> = true` for
  each field in the *effective* field set.
- **Effective fields** (`binding.rs` `effective_fields`, ~97). Explicit
  `fields:` is used verbatim. Defaulted sets are presence-filtered, but
  `image` always survives (Q1 parity).
- **Default field sets** (`config.rs` `apply_type_defaults`, ~926). Default and
  Grid both include `image`, `title`, `subtitle`, `author`, `date`,
  `description`, `categories`, `reading-time`, `filename`, `file-modified`.
  So gating on `show.*` changes nothing for listings without `fields:`, which
  answers the strand's "check the default includes image" item.
- **Quarto 1** (`external-sources/quarto-cli/src/resources/projects/website/listing/`).
  - `item-grid.ejs.md`: image block under `fields.includes('image')`
    (placeholder when the item has none); every other field under
    `showField(f)` = `fields.includes(f) && item[f] !== undefined`; categories
    under `fields.includes('categories') && item.categories`.
  - `item-default.ejs.md`: the same idea with `fields.includes(...)` checks
    (title/subtitle/description gated on the field alone; date/author/
    reading-time on field and value).
  - Q1 also has structure q2's templates lack: the grid omits `.card-body`
    when no body field shows; a `.card-footer` for `filename` /
    `file-modified`; an "other fields" table (grid) or metadata list
    (default) for any field without a dedicated slot; author/date flex
    justification classes; `image-align`. None of that is in this strand.
- **Existing tests.** `transforms/listing_render.rs` has template-level unit
  tests (e.g. `item_default_renders_category_chips_when_categories_field_enabled`)
  and a `show.*` test for custom templates (~1250). No test covers
  `fields:` narrowing for the built-in grid/default templates.

## Decisions (2026-10-09)

1. **Scope: gating only**, with the structural parity filed as
   bd-x7c196m3 and planned to follow directly, possibly in the same
   session. So this strand should not paint that work into a corner (see
   "Leaving room for bd-x7c196m3").
2. **Sequencing: (a).** Land this first, in parallel with the
   description-precedence branch, which rebases onto it. The conflict is in
   the description and image blocks of both templates.
3. **Placeholder parity: keep it.** `image` in the field set means a
   placeholder for every image-less item, as in Q1.

## Leaving room for bd-x7c196m3

- **Doctemplates have no boolean `or`.** Q1's
  `showField('title') || showField('subtitle') || ...` (omit an empty
  `.card-body`) and the author/date justify class can't be written in the
  template. They have to be keys computed in `binding.rs` (e.g.
  `show-card-body`, `attribution-justify`). This strand should establish
  that pattern rather than nest more template conditionals.
- **`show.<field>` keeps its current meaning** ("the effective field set
  contains it"), because custom templates read it with Q1 semantics. Q1's
  `showField` (in the set *and* the item has a value) is a different
  predicate; if bd-x7c196m3 needs it per field, add it as a separate key
  rather than changing `show`.
- **Naming next to the sibling branch.** That plan adds a top-level
  `show-description` key (description *or* envelope). It is easy to
  confuse with `show.description`. Mention this to that agent when it
  rebases.

## Diagnostic for missing fields (exploratory, 2026-10-09)

The user asked whether the current source-mapping infrastructure could
support a diagnostic like "37 listing items are missing an `image` field:
foo.qmd, bar.qmd, ..." with truncation. Findings:

**What exists.**
- *Many labels on one diagnostic.* `DiagnosticMessageBuilder::add_detail_at`
  / `add_info_at` attach a `SourceInfo` per detail, and the Ariadne
  renderer draws detail labels in files other than the primary one
  (`quarto-error-reporting` `diagnostic.rs` ~1256).
- *Truncated file lists.* `quarto-error-reporting::coalesce` already
  renders an affected-files list capped at `AFFECTED_FILES_CAP = 3` with an
  "(and N others)" tail. It is used for the cross-page Q-14-1 grouping. A
  listing diagnostic would build its own list in the message text but
  should use the same wording and cap style.
- *A listing-level anchor.* `Listing.categories_source` captures the YAML
  span of `categories:` for Q-12-12 ("categories enabled but no item has
  any"), which is the closest precedent: one warning per listing about a
  property of all its items. A matching `fields_source` is a few lines in
  `config.rs` (~479), where `entry.value` carries the span.
- *Item identity.* Document items carry `ItemTarget::Document { source_path }`
  (project-relative), and records carry their YAML `SourceInfo`
  (`record.rs`). The emit site would be `transforms/listing_generate.rs`,
  which already pushes per-item diagnostics (Q-12-26) while hydrating.

**What doesn't exist.**
- *Per-item front-matter spans for documents.* `DocumentProfile` keeps
  `SourceInfo` for `aliases:` and `resources:` only. Q-12-26 passes `None`
  for document items for this reason. Pointing at each item's front matter
  would need a profile field (a version bump, which conflicts with the
  sibling branch's 14 → 15 bump) **and** the cross-document rebasing that
  `website_post_render.rs` `locate_alias` does: every document's span is
  rooted at its own `FileId(0)`, and the listing host's `SourceContext`
  holds only the host file. That is real work, and for "this field is
  *absent*" there is no span to point at anyway.

**So:** listing-anchored diagnostic with the offending files named in the
text (truncated) is cheap and fits the existing pieces. Per-item Ariadne
spans are not worth it for an absent field.

**When should it fire?** Q1 is silent. Defaulted grid/default field sets
always contain `image`, so "warn when any item lacks a field" would fire
on most blogs, where a placeholder per image-less post is intended. Options:

- (i) Warn only when **no** item has an image and no `image-placeholder:`
  is set: every card is an empty grey bar. This is the plans-page case and
  mirrors Q-12-12. Naming files is pointless here, since it's all of them.
- (ii) Warn when `image` is **author-explicit** in `fields:` and some items
  lack one, naming them, truncated.
- (iii) Implement Q1's **`field-required:`**. q2 parses it
  (`config.rs` ~494, `Listing.field_required`) but never enforces it. Q1
  (`website-listing-read.ts` ~972) throws on the *first* item missing a
  required field. q2 could make it one aggregated error per listing and
  field, naming the offending items. That is the opt-in, general form of
  the user's example.

## Decisions, round 2 (2026-10-09)

- **No warning in this strand.** The empty-placeholder warning (option (i)
  below) turned out to need a two-stage design and was deferred to
  bd-f9unh898 (P4, for a diagnostics-focused pass). Its design notes stay
  below for that strand.
- **(iii) is filed** as bd-9q7w7xhq (enforce `field-required:`, aggregated
  per listing and field, offending items named with a truncated list).
- (ii) is dropped.
- **Go-ahead given** for the gating work.

## Deferred: empty-placeholder warning (bd-f9unh898)

**It can't be decided when the items are built.** An item with no authored
`image:` gets an L7 image envelope (`helpers::image_placeholder_begin`,
carrying listing id, item index and href). After render,
`post_render_upgrade/substitute.rs` `substitute_images` fills it with the
rendered page's preview image, falling back to `image-placeholder:` and
then to the empty div. A blog whose posts have body images but no `image:`
front matter ends up with real thumbnails. A warning based only on
item data would fire falsely there, which is the common case.

What `listing_generate.rs` *can* know:
- whether `image` is in the effective field set;
- whether any item has an authored image (`image-html` branch, no
  envelope);
- whether `image-placeholder:` is set;
- whether any placeholder can be filled at all (document items get
  envelopes; plain records don't).

What only L7 knows: whether every envelope stayed empty.

**Two-stage shape (proposed).** At generate time, a listing is a
*candidate* when `image` is shown, no item has an authored image, and no
`image-placeholder:` is set. Then:
- If no item can be filled (records only), warn right there, with a
  source span (see below).
- Otherwise mark the candidate's envelopes (e.g. a flag in the marker's
  attrs). L7 groups envelopes by listing id within a host file and warns
  once when every flagged envelope stayed empty.

L7 doesn't run in the hub client (`orchestrator.rs` ~566 is
`not(wasm32)`), so there the warning only appears in the records-only case.
That's acceptable.

**Location.** L7 diagnostics carry no source span today: Q-12-13 uses
`SourceInfo::generated(By::unknown())`. The L7 warning would name the
listing page and listing id in its text, plus the hint (set
`fields:` without `image`, or set `image-placeholder:`). The generate-time
(records-only) case can anchor on the listing, via a new
`Listing.fields_source` captured like `categories_source` (`config.rs`
~479 / ~577). When `fields:` is defaulted there is no `fields:` span, so it
falls back to the listing's own span.

**Interaction with the description-precedence branch.** That branch moves
placeholder gating from item origin to `image_source` (Derived/Absent),
puts an envelope inside the `image-html` branch too, and gives L1 a
derived (first body) image. The candidate test should then read "no
*authored* image" (`image_source.is_authored()`), and a derived image
counts as filled. Whichever branch lands second adapts; the flag-in-marker
approach survives that change.

## Decisions, round 3 (2026-10-09, during implementation)

- **`type: custom` without `fields:` gets Quarto 1's default:** every field
  at least one item carries (`binding.rs` `fields_items_carry`, mirroring
  Q1's `defaultFields(Custom, itemFields)`). Before, custom listings had an
  empty field set, so every `show.*` was false. Once the item templates
  gate on `show.<field>`, a custom template wrapping `$items:item-default()$`
  without `fields:` rendered empty cards (caught by
  `custom_template_using_item_default_partial_emits_l7_envelopes`).
- **The other default-set differences are left alone here** and filed as
  bd-n7g28c3o. q2's grid/default sets include `subtitle` (grid),
  `categories`, `reading-time`, `filename` and `file-modified`, which Q1's
  don't, and Q1 gives records-only listings every item field. That has to
  be settled before bd-x7c196m3 adds the filename/file-modified footer.

## Implementation todo

### Phase 0 — Tests first (red)
- [x] `listing_render.rs` `grid_and_default_render_only_explicit_fields`:
  grid and default with `fields: [date, title, description]` render no
  image or placeholder, subtitle, author, reading time or categories.
- [x] `grid_and_default_render_every_field_by_default`: without `fields:`,
  everything renders, including the placeholder for an image-less item.
- [x] Red: the explicit-fields test failed on the image (`IMG-a` rendered).
  The defaults test passed, as expected.

### Phase 1 — Gate the templates
- [x] `item-grid.template` and `item-default.template`: every block wrapped
  in `$if(show.<field>)$` (image and placeholder under `show.image`).
- [x] Custom default field set (round 3), with
  `binding.rs` `custom_listing_without_fields_uses_fields_items_carry`.
  `custom_template_sees_listing_fields_and_per_item_show` now sets
  `fields_explicit`, as config parsing does.

### Phase 2 — Verification
- [x] `cargo xtask verify --skip-hub-build`: all 16032 Rust tests pass.
  Hub-client WASM tests failed against a stale `wasm_quarto_hub_client`
  build (2026-09-22; `wasm.get_typst_assets is not a function`), so a full
  `cargo xtask verify` with the hub build is the real check.
- [x] Repro: narrow rows match Q1 (README).
- [x] Full `cargo xtask verify` (hub build, fresh WASM): all green.
- [x] Plans page with `type: grid` and `fields: [date, title, description]`
  (temporary edit, reverted): 1054 cards, 0 placeholders, 0 `card-img-top`.
  A single-file `q2 render --strict plans/index.md` in grid mode reports one
  Q-12-13, because one plan's sibling HTML was missing from `_site`. That
  comes from the description envelope (grid cards have one, table cells
  don't), not from gating; it is the description-precedence branch's area.

### Phase 3 — Docs
- [x] `listings.qmd`: new "Fields" section; `show.<field>` entry updated
  for the custom default.

## Risks / tradeoffs (draft)

- **Merge conflict** with the description-precedence branch on both
  templates (Decision 2).
- **Custom field sets that omit `title`** now render cards without titles.
  That is Q1 behaviour and what the author asked for, but it is a visible
  change for anyone relying on the current behaviour.
- **Snapshot churn** should be small: explicit `fields:` in existing fixtures
  is mostly on table listings.

# Table listings ignore fields/field-display-names (bd-listing-table-fields-peg1w3b3)

**Date:** 2026-08-09
**Braid:** bd-listing-table-fields-peg1w3b3 (bug, P1, label `listings`)
**Branch:** `main` (investigated in place; no worktree created)
**Status:** Design settled with user (2026-08-09) — implementation in progress.
**Follow-up:** bd-bl1e00r6 (sort-ui/filter-ui/table-hover interactive parity, discovered-from this strand)

## Problem

The built-in table listing templates are static: `listing-table.template` hardcodes `| Title | Date | Author |` and `item-table.template` hardcodes `| [$title$]($path$){.no-external} | $date$ | $author$ |`. Consequences:

- `fields:` is ignored — a `fields: [title]` listing still renders three columns.
- `field-display-names:` is ignored for headers.
- Items missing `date`/`author` produce "Undefined variable" doctemplate diagnostics, surfaced as one Q-12-10 warning per listing (the binding deliberately omits absent optional fields so `$if(field)$` works).

Reproduced at HEAD (`main` @ 4bb32844) with the committed repro under `claude-notes/plans/listing-table-fields-investigation/repro/` — rendered output was inspected: `<th>Title</th><th>Date</th><th>Author</th>` + `Q-12-10 … Undefined variable: date`.

doctemplate is Pandoc-style/logic-less (no `item[field]` indexing), so no static template can render an author-chosen column set — the dynamism must come from Rust.

## Settled design (user-aligned 2026-08-09)

1. **Mechanism (A): pre-rendered binding keys.** The binding computes `listing.table-header` (markdown header + separator rows) and a per-item `table-row` markdown string; the two table templates shrink to interpolating those. Additive to the L8 binding contract; follows the `image-html`/`category-html` pre-rendered-helper precedent. Templates stay static/readable; L8 shadowing untouched.
2. **Header names: Q1 parity.** Built-in default display-name map overlaid with the author's `field-display-names`; unknown fields fall back to the **raw field name**. Q1's defaults (from `_language.yml`, English hardcoded for now — Q2 has no format.language yet): `image → " "`, `date → "Date"`, `title → "Title"`, `description → "Description"`, `author → "Author"`, `filename → "File Name"`, `date-modified → "Modified"`, `file-modified → "Modified"`, `subtitle → "Subtitle"`, `reading-time → "Reading Time"`, `word-count → "Word Count"`, `categories → "Categories"`.
3. **Cell rendering: Q1 parity** (from `listing-table.ejs.md` `readField`/`outputValue`/`outputLink` + `website-listing-read.ts`):
   - `image` → the existing `image-html` helper output.
   - Curated fields read from the item struct: `title`, `subtitle`, `description`, `author`, `authors` (join `", "`), `date`/`date-modified` (pre-formatted, as the binding already does), `categories` (join `", "`), `reading-time` (`"N min read"`), `word-count`, `filename`.
   - Unknown fields → dotted-path lookup into the item's `extra` map (`a.b` walks nested maps, Q1 `readField` parity); array values join `", "`.
   - Missing value → **empty cell** (Q1 emits `&nbsp;`; empty cell is the markdown-table equivalent).
   - **`field-links`**: new config option, Q1 default for table listings is `[title, filename]` (empty for other types). A linked field's cell becomes `[<value>](<path>){.no-external}` when the item has a path and the value is non-empty. (Q1's extra `listing-<field>` classes exist to serve list.js — deferred to bd-bl1e00r6.)
4. **Cell escaping:** escape `|` → `\|` and flatten newlines to spaces inside cell values; accept that some documents need markdown changes for Quarto 2. Values otherwise pass through as markdown (consistent with the other templates' `$title$` interpolation).
5. **Interactive parity (sort-ui anchors, filter-ui, table-hover onclick, list.js classes) is out of scope** → filed as bd-bl1e00r6.

### Additional Q1-parity findings from the source study

- **Default table field order is `[date, title, author]`** (`kDefaultTableFields`) — Q2's `apply_type_defaults` already matches, but the hardcoded template header order (Title|Date|Author) doesn't. Post-fix, default tables render **Date | Title | Author**. This is a deliberate parity change; expect snapshot churn (must be documented per snapshot policy).
- **Q1 filters *default* (not author-explicit) fields by presence in items** (`website-listing-read.ts:578` — keep `image` always; if the filter empties the list, fall back to all item fields). Author-explicit `fields:` is used verbatim. This is what prevents an all-empty Author column on default tables. Q2 applies type defaults at config-parse time with no item knowledge, so implementing this needs an explicit-fields flag on `Listing` and a presence-filter at render time (items are known in `render_one`).

## Implementation notes

- Config: `crates/quarto-core/src/project/listing/config.rs` — parse `field-links` (string list) into `Listing::field_links`; add `fields_explicit: bool` set when the author supplied `fields:`; `apply_type_defaults` fills `field_links` (`[title, filename]` for Table, `[]` otherwise).
- Binding: `crates/quarto-core/src/project/listing/binding.rs` (+ possibly `helpers.rs`) — display-name resolution, cell rendering, `table-header` on the listing map, `table-row` on the item map. Computed unconditionally (cheap; custom templates may use them).
- Presence filter: `crates/quarto-core/src/transforms/listing_render.rs::render_one` (or just before `build_listing_context`) — when `!fields_explicit`, filter `listing.fields` to fields present in ≥1 item (keeping `image`), Q1 fallback semantics.
- Templates: `templates/listing-table.template` → wrapper + `$listing.table-header$` + items loop; `templates/item-table.template` → `$table-row$`. Keep the `item-table` partial so L8 shadowing still works.
- The re-parse path means pre-rendered markdown strings in the binding "just work" (template output is parsed as qmd).

## Work items

### Phase 0 — Tests (TDD: written first, verified failing)

- [ ] Survey existing listing test layout (binding unit tests, listing_render tests, any project-render integration fixtures) and decide where each new test lives.
- [ ] e2e/regression: `fields: [title]` + `field-display-names: {title: "How To"}` table listing renders exactly one `How To` column and **zero** Q-12-10 warnings (drive the real render path per end-to-end verification policy).
- [ ] Missing-value tolerance: `fields: [title, date]` where one item lacks `date` → empty cell, no diagnostic.
- [ ] Default table (no `fields:`): Date | Title | Author order; and with items lacking `author`, the Author column is dropped (presence filter); author-explicit `fields:` is never presence-filtered.
- [ ] Binding unit tests: `table-header` (display-name overlay, raw-name fallback, `image → " "` header), `table-row` (curated fields, dotted-path `extra` lookup, array join, empty cell for missing, `\|` escaping, newline flattening), `field-links` linking behavior (title linked by default; non-linked field plain; `field-links: []` unlinks title).
- [ ] Config unit tests: `field-links` parsing, table default `[title, filename]`, `fields_explicit` flag.
- [ ] Run new tests, confirm they fail for the expected reason before implementing.

### Phase 1 — Config

- [ ] Parse `field-links` into `Listing::field_links: Vec<String>` (Q-12-5-style diagnostic on wrong type, consistent with neighbors).
- [ ] Add `fields_explicit: bool` (set in the `"fields"` parse arm).
- [ ] `apply_type_defaults`: default `field_links` per type.
- [ ] Phase-1 unit tests pass.

### Phase 2 — Binding

- [ ] Display-name resolution helper (defaults map + author overlay + raw fallback).
- [ ] Cell renderer: per-field value lookup (curated → struct, unknown → dotted-path `extra`), image special-case, join rules, escaping, `field-links` wrapping.
- [ ] `listing.table-header` + per-item `table-row` keys wired into `build_listing_map`/`build_item_map`.
- [ ] Phase-2 unit tests pass.

### Phase 3 — Templates + presence filter

- [ ] Rewrite `listing-table.template` / `item-table.template` to consume the new keys.
- [ ] Presence-filter default fields in the render transform (author-explicit fields verbatim).
- [ ] Full workspace test run; review + document every changed `.snap` (expect table-column-order churn).

### Phase 4 — End-to-end verification + docs

- [ ] `cargo run --bin q2 -- render` on the committed repro; inspect `_site/index.html` for the single `How To` column and absence of Q-12-10; record invocation + output snippet here.
- [ ] Render `docs/` with q2 and spot-check any table listings.
- [ ] Update `docs/` listing documentation if it describes table columns / `field-display-names` / `field-links`.
- [ ] `cargo xtask verify` (full — quarto-core changes affect the WASM leg).

## Risks / tradeoffs

- The binding is the **load-bearing L8 public contract**: `table-header`/`table-row` are additive (safe), but custom templates that shadow `item-table` keep receiving the full item map — unchanged shape, no break expected. Call out the new keys in the commit.
- Default-table column order changes to Date | Title | Author (Q1 parity). Snapshot churn must be reviewed and documented per CLAUDE.md snapshot policy.
- Presence filtering changes `show.*` flags for default/grid listings whose items lack curated fields — aligns with Q1, but worth watching in snapshot review.
- English-hardcoded default display names; revisit when Q2 grows language/i18n support.

## Investigation record (2026-08-09)

- Repro: `claude-notes/plans/listing-table-fields-investigation/repro/` — `cargo run --bin q2 -- render .` → `Warning [Q-12-10]: Listing 'guides' doctemplate produced 4 diagnostic(s); first: Undefined variable: date`; `_site/index.html` shows `<th>Title</th><th>Date</th><th>Author</th>` despite `fields: [title]` + `field-display-names`. Output inspected directly.
- Q1 references: `external-sources/quarto-cli/src/resources/projects/website/listing/listing-table.ejs.md` (template loop, readField/outputValue), `src/project/types/website/listing/website-listing-template.ts` (fieldName/outputLink utilities), `src/project/types/website/listing/website-listing-read.ts` (defaultFieldDisplayNames, kDefaultFieldLinks, kDefaultTableFields, suggested-field presence filter), `src/resources/language/_language.yml` (listing-page-field-* strings).

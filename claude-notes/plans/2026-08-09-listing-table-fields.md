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

- [x] Survey existing listing test layout — homes chosen: `binding.rs` test mod (unit), `transforms/listing_render.rs` test mod (transform + diags), `tests/integration/listing_pipeline.rs` (e2e via `render_project`). Per-file warnings don't reach `ProjectRenderSummary`, so the no-Q-12-10 assertion lives at the transform level (`run_transform` returns diags).
- [x] e2e: `table_fields_and_display_names_render_single_column` (+ transform-level `table_fields_subset_renders_single_column_without_diagnostics` for the zero-diagnostics half).
- [x] Missing-value tolerance: `table_row_missing_value_renders_empty_cell`.
- [x] Default table order + presence filter: `table_default_fields_presence_filtered_and_ordered` (e2e), `defaulted_table_fields_presence_filtered_when_no_author`, `explicit_fields_never_presence_filtered`, `presence_filter_keeps_image_field` (binding).
- [x] Binding unit tests: header overlay/raw-fallback/image-blank; row link+date, escaping, newline flattening, dotted-path extra, array join, authors/categories join, reading-time, image cell, field-links unlink/filename.
- [x] Config unit tests: field-links defaults (table/non-table), explicit-empty survives defaults, explicit list, `fields_explicit` true/false/empty-list.
- [x] Scaffolding so the batch compiles: `Listing.field_links` → `Option<Vec<String>>` (None = unspecified), new `Listing.fields_explicit: bool` (both behavior-neutral).
- [x] Ran the batch: 18+ tests fail for the expected reasons (missing `table-header`/`table-row` keys, Q-12-10 diags present, hardcoded columns in HTML); 4 pass as scaffolding-covered regression guards.

### Phase 1 — Config

- [x] `field-links` was already parsed (never consumed); re-typed as `Option<Vec<String>>` so explicit `field-links: []` survives defaulting.
- [x] `fields_explicit: bool` set in the `"fields"` parse arm (explicit-but-empty list counts as defaulted).
- [x] `apply_type_defaults` fills `field_links` (`[title, filename]` for Table, `[]` otherwise).
- [x] Phase-1 unit tests pass.

### Phase 2 — Binding

- [x] `default_display_name` (Q1 `_language.yml` map, English) + `display_name` overlay with raw-name fallback.
- [x] Cell renderer: `item_field_display_value` (curated → struct fields, unknown → `extra_field_value` literal-then-dotted lookup, arrays join `", "`), image → `image-html`, `escape_table_cell` (`\|`, newline flattening), `field-links` wrapping as `[value](path){.no-external}`.
- [x] `listing.table-header` + `listing.field-links` + per-item `table-row` wired in; `effective_fields` presence filter lives in `build_listing_context` (binding layer, not the render transform — shared by every consumer incl. WASM) and feeds `listing.fields`, `show.*`, header and rows uniformly.
- [x] Phase-2 unit tests pass.

### Phase 3 — Templates + presence filter

- [x] `item-table.template` → `$table-row$`; `listing-table.template` → `$listing.table-header$` + a `$for(items)$ $it:item-table()$ $endfor$` loop. The for-loop (not `$items:item-table()$`) is load-bearing: the doctemplate resolver chomps a partial's final newline (Pandoc `removeFinalNl` parity), so bare iterated application merges all rows onto one line. **This was a latent bug in the old template too** — any table listing with ≥2 items merged its rows into a single `<tr>`; the block-based default/grid templates survive the same chomp only by div-fence arity absorption (`:::` + `:::` → a longer valid fence). Row-structure assertions (`<tr` count) added to both e2e tests.
- [x] Presence filter (implemented in binding, see Phase 2).
- [x] Full workspace test run: 11179 passed, 197 skipped. **Zero snapshot churn** — no existing `.snap` covered table listing markup.

### Phase 4 — End-to-end verification + docs

- [x] Real-binary verification on the committed repro:
  - Invocation: `cargo run --bin q2 -- render .` in `claude-notes/plans/listing-table-fields-investigation/repro/`
  - Output inspected (`_site/index.html`): single `<th>How To</th>`, one `<tr>` per item with linked titles (`<a href="one/index.html" class="no-external">First guide</a>`), **zero warnings** (previously: three hardcoded columns + `Q-12-10 … Undefined variable: date`).
- [x] Rendered `docs/` with q2 (189/189 files). The error-catalog index (`docs/errors/index.qmd`, itself a `type: table` listing with `fields`/`field-display-names`/`field-links`) now renders its four declared columns with correct headers and linked titles. Its `code`/`subsystem`/`status` cells are empty because those are bare top-level frontmatter keys, which Q2's profile contract only routes into `ListingItem.extra` via an explicit `listing-item.extra:` opt-in — a deliberate design boundary (see `document_profile.rs`), **not** regressed by this change (those columns never rendered before either). Filed as **bd-0t4e07jk** (discovered-from) with the design options rather than deciding unilaterally.
- [x] docs/ prose: no user-facing page documents table listing columns yet beyond Q-12-5's `field-display-names` description, which stays accurate; nothing to update for this fix.
- [x] `cargo xtask verify` (full incl. WASM/hub-client legs) passed on the final tree. One clippy `-D warnings` finding (`map_unwrap_or`) fixed along the way. Side effect: the WASM crate's standalone `Cargo.lock` picked up the 0.13.0 → 0.14.0 workspace version bumps (leftover from the v0.14.0 release; the excluded-from-workspace lockfile only refreshes when the WASM leg builds).
- [x] Pre-commit review checklist (`claude-notes/instructions/review.md`): no new HashMap/FxHashMap imports (new maps are `BTreeMap`/ordered slices), no `#[serde(flatten)]`, no TODOs, `cargo fmt --check` clean, clippy clean, 11179 workspace tests + full verify green, zero snapshot changes. Staged; awaiting user approval to commit.

## Session log

- **2026-08-09 (session 1):** investigation, design alignment, full implementation (Phases 0–3 complete, Phase 4 verification done except final `cargo xtask verify` + commit). Discovered + filed: bd-bl1e00r6 (interactive table parity), bd-0t4e07jk (bare-frontmatter listing fields).

## Risks / tradeoffs

- The binding is the **load-bearing L8 public contract**: `table-header`/`table-row` are additive (safe), but custom templates that shadow `item-table` keep receiving the full item map — unchanged shape, no break expected. Call out the new keys in the commit.
- Default-table column order changes to Date | Title | Author (Q1 parity). Snapshot churn must be reviewed and documented per CLAUDE.md snapshot policy.
- Presence filtering changes `show.*` flags for default/grid listings whose items lack curated fields — aligns with Q1, but worth watching in snapshot review.
- English-hardcoded default display names; revisit when Q2 grows language/i18n support.

## Investigation record (2026-08-09)

- Repro: `claude-notes/plans/listing-table-fields-investigation/repro/` — `cargo run --bin q2 -- render .` → `Warning [Q-12-10]: Listing 'guides' doctemplate produced 4 diagnostic(s); first: Undefined variable: date`; `_site/index.html` shows `<th>Title</th><th>Date</th><th>Author</th>` despite `fields: [title]` + `field-display-names`. Output inspected directly.
- Q1 references: `external-sources/quarto-cli/src/resources/projects/website/listing/listing-table.ejs.md` (template loop, readField/outputValue), `src/project/types/website/listing/website-listing-template.ts` (fieldName/outputLink utilities), `src/project/types/website/listing/website-listing-read.ts` (defaultFieldDisplayNames, kDefaultFieldLinks, kDefaultTableFields, suggested-field presence filter), `src/resources/language/_language.yml` (listing-page-field-* strings).

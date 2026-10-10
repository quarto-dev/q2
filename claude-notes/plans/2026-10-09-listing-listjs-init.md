---
title: 'Listings: no List.js init script emitted — quarto-listing.js is inert (bd-nbv80e33)'
date: 2026-10-09
description: 'Q2 ships list.min.js and quarto-listing.js but never instantiates List.js, so pagination, category filtering and the no-matching reveal are dead; this scopes emitting the init script plus the item/page markup it depends on.'
status: implemented  # All phases done on braid/bd-nbv80e33-listing-listjs-init; awaiting review/merge
braid:
  strand: bd-nbv80e33
---

**Branch:** `braid/bd-nbv80e33-listing-listjs-init` (topic branch in the main checkout, based on `main` @ `ea72d68aa`)
**Design aligned 2026-10-09 — see § Decisions. Implemented 2026-10-09; awaiting review/merge.**

## Triage verdict

**Ready to design.** The gap reproduces at HEAD exactly as described. It is
also wider than the strand title says: the init script is the visible
hole, but the markup the script and `quarto-listing.js` read is missing as
well (`data-index`/`data-categories` on items, pagination `<nav>`, sort and
filter controls). The design questions below are mostly about how far this
strand should reach into that surrounding markup.

## Issue context

P3 bug, `listings` label, filed 2026-08-20 by Carlos Scheidegger while
fixing bd-listing-ellipsis-no-matching-l963osy1. Q2 links
`site_libs/listing/list.min.js` and `quarto-listing.js`
(`listing_render.rs::register_listing_js_artifacts`) but never emits Q1's
per-listing inline script:

```js
window['quarto-listings']['listing-<id>'] = new List('listing-<id>', options);
window['quarto-listing-loaded']();
```

So `window['quarto-listings']` is never populated, and everything driven
from `quarto-listing-loaded()` is dead code: pagination, category filtering
(`quartoListingCategory`), progressive images, and the
`.listing-no-matching` reveal.

Gordon Woodhull's comment (2026-08-26) says the doc comment on
`helpers.rs::metadata_attrs` ("the list.min.js sort/filter UI is gated on
these attrs") is stale. No built-in template emits `$metadata-attrs$`, and
nothing in-tree reads `data-index`/`data-categories`. The comment should be
fixed with this strand.

## Dependency graph

- **discovered-from**: bd-listing-ellipsis-no-matching-l963osy1 (closed, PR
  #570). That strand added the hidden `.listing-no-matching` div for markup
  parity only. Its design session (2026-08-20, decision 2) explicitly
  deferred the List.js wiring to this strand.
- **related (by description, no edge)**: bd-bl1e00r6 (open, P2), table
  listings\' sort-ui/filter-ui/table-hover parity: sortable headers, row
  onclick, `listing-<field>` cell classes. It notes that interactive tables
  probably need a raw HTML table instead of a pipe table. This strand is
  the shared bootstrap that bd-bl1e00r6 would sit on.
- No incoming `blocks` edges, so nothing open is waiting on this. Urgency
  comes only from user-visible parity: categories in the sidebar look
  clickable and do nothing, and long listings never paginate.

## What the code looks like today

Repro: `claude-notes/plans/listing-listjs-init-investigation/repro/`. It has
3 posts, `page-size: 2`, `categories: true`, `sort-ui: true` and
`filter-ui: true`. Rendered at HEAD (`ea72d68aa`), `_site/index.html` shows:

| Q1 emits | Q2 at HEAD |
|---|---|
| inline `<script>` in header: `new List('listing-<id>', {valueNames, page, pagination, searchColumns})` (`website-listing-template.ts::templateJsScript`, 520–590) | **absent** (only the two `<script src>` tags) |
| container id `listing-${listing.id}`; with Q1's default id `listing` that gives `#listing-listing`, and with several listings `#listing-listing-1`… | `#listing-1` (Q2's synthesized id, no prefix) |
| per-item `data-index='N' data-categories='<b64>' data-listing-<f>-sort='…'` (`utilities.metadataAttrs`) | **absent**; `helpers::metadata_attrs` exists and is bound as `$metadata-attrs$`, but no template uses it, and it emits categories **raw**, while `quarto-listing.js::filterListingCategory` calls `atob()` on them |
| `<nav id="<id>-pagination" class="listing-pagination"><ul class="pagination">` when `page-size < items.length` (`_pagination.ejs.md`) | **absent** |
| `.listing-actions-group` with a sort `<select>` (calls `quarto-listings[...].sort(...)`) and a filter `<input class="search">` (`_filter.ejs.md`) | **absent**; `sort-ui`/`filter-ui` are parsed (`config.rs`) and bound (`binding.rs:169–172`) but no template reads them |
| `.listing-no-matching.d-none` | present (PR #570) |
| category sidebar + `onclick="window.quartoListingCategory('<b64>')"` | present |

Other facts the design depends on:

- Q2's `.list` div's direct children are the item wrappers
  (`.quarto-post` for default, `.g-col-1` for grid), which is the shape
  List.js `parse()` expects. The table listing has no `.list` at all; it
  emits a markdown pipe table (see bd-bl1e00r6).
- Item value classes that List.js `valueNames` would read already exist
  in the default and grid templates: `listing-title`, `listing-subtitle`,
  `listing-date`, `listing-author`, `listing-reading-time` and
  `listing-description`. A missing class returns `""` to List.js and is
  harmless.
- Page-size defaults differ. Q1 uses table 30, grid 18 and default 25
  (`website-listing-read.ts:597`; the `|| 50` in `templateJsScript` is a
  dead fallback — *corrected 2026-10-09; the first draft said 50*).
  Q2 uses `config.rs:105` `page_size: 25` and a type preset at
  `config.rs:988` (30). These need reconciling once pagination actually
  takes effect, or Q2 sites will start paginating at a different count
  than Q1.
- `filter-ui`/`sort-ui` default to `true` for tables in Q1 and to `false`
  everywhere in Q2, except the table preset at `config.rs:986`.
- Q2 has no "inline page script" mechanism today. JS reaches pages only as
  `js:`-keyed file artifacts. Options are a raw-HTML `<script>` block
  spliced into the listing's AST (next to the no-matching div) or a
  `header-includes` push. Q1 uses `include-in-header`.

## Decisions (design session 2026-10-09)

1. **Scope:** bootstrap + pagination here (init script, item `data-*`
   attrs, pagination nav). Sort/filter *controls* are
   **bd-m6wglib4**, to be picked up immediately afterwards — so build
   the bootstrap complete enough that the follow-up only adds the
   controls: full Q1 `valueNames` (including the
   `listing-<field>-sort` data targets) and `searchColumns` land here.
2. **Targets:** `q2 render` and `q2 preview --static` only. The
   hub-client React renderer gets a native listing component later
   (**bd-4dfdo8vi**). Survey of hub-client: neither preview mode
   executes a `<script>` inside a body raw-HTML block (React inserts it
   via `innerHTML`; the full-HTML iframe has no `allow-scripts`), and
   the llms view drops raw HTML. Emission is still gated on the
   pipeline profile (`HtmlRender`/`RevealjsRender`) so the preview AST
   never carries it.
3. **Custom listings:** no List.js (no init script, no pagination nav)
   — keeps the door open for React-driven custom listings. Table
   listings also get none until bd-bl1e00r6 gives them a `.list`.
4. **Page sizes:** Q1's defaults. *Correction to the investigation
   notes:* Q1's real defaults are **25 / 18 / 30** (default / grid /
   table, `defaultPageSize` in `website-listing-read.ts`); the `|| 50`
   in `templateJsScript` is a dead fallback. So only grid changes
   (25 → 18). Also fixed: Q2's table preset overwrote author-set
   `page-size` / `sort-ui` / `filter-ui`.
5. **Container id:** Q1's `listing-<id>`, with Q1's synthesized id
   `listing` for a lone listing (`#listing-listing`); arrays keep
   `listing-N` (`#listing-listing-1`). An author's explicit slot
   `::: {#posts}` is renamed to `#listing-posts`, as Q1 does.
6. **Verification:** HTML-string tests here; browser-level tests are
   **bd-rlxlrja3** (after the sort/filter strand).

## Work items

### Phase 1 — Config defaults (Q1 parity)

- [x] Tests: per-type page-size defaults, author values surviving the
      table preset, Q1 `kDefaultFieldTypes` merge, Q1 field-sort
      defaults, single-listing synthesized id `listing`.
- [x] `page_size` / `sort_ui` / `filter_ui` become `Option`s resolved
      by `Listing::{page_size, sort_ui, filter_ui}()`; table preset no
      longer clobbers author values.
- [x] `apply_type_defaults` merges Q1's default field types and fills
      `field_sort`.
- [x] `SINGLE_LISTING_ID = "listing"` for map / string / boolean forms.
- [x] Fix up downstream tests that assumed `listing-1` for a lone
      listing.

### Phase 2 — Item metadata attrs

- [x] Tests: `metadata_attrs` emits `data-index`, b64
      (`btoa(encodeURIComponent)`) `data-categories`, and
      `data-listing-<field>-sort` for date / number / minutes typed
      fields; values survive the qmd re-parse on the item wrapper.
- [x] Rewrite `helpers::metadata_attrs` (takes the listing for field
      types); fix the stale "gated on these attrs" doc comment
      (Gordon's comment on the strand).
- [x] Splice `$metadata-attrs$` into the `item-default` wrapper and the
      `item-grid` outer `.g-col-1` div (the direct children of
      `.list`).
- [x] Update `listings.qmd` § metadata-attrs and `listing-templates.qmd`
      (porting table row, "none of the built-ins emit it" notes).

### Phase 3 — Container id and classes

- [x] Tests: implicit container `#listing-<id>`; explicit slot renamed
      to `#listing-<id>`; both carry `quarto-listing` +
      `quarto-listing-container-<type>`; second pass stays idempotent;
      ids `x` / `listing-x` don't capture each other's containers;
      llms view still finds the container.
- [x] `Listing::container_id()` used by `listing_render.rs` and
      `llms.rs`; `ListingType::name()` replaces binding's private copy.
- [x] Slot walk: rendered container (container id + marker) ⇒
      AlreadyRendered; author slot = listing id, not a section, not a
      rendered container.
- [x] **Q-12-25 retired.** Its premise (listing and section claim one
      anchor) disappears once the container is `listing-<id>`; emission
      and `find_colliding_section` removed, page marked `deprecated`
      per `docs/errors/README.md` (catalog entry stays). This also
      resolves the "duplicate-id residue" noted when
      bd-listing-id-collides-with-heading-l57w41jl closed.
- [x] Docs: `Q-12-4.qmd` (synth ids), `Q-12-25.qmd` (retired),
      `listing-templates.qmd` (container id/classes).

### Phase 4 — Init script + pagination nav

- [x] Tests (`listjs.rs`): Q1 `valueNames` order (fields as
      `listing-<f>`, `{data:['index']}`, `{data:['categories']}`, sort
      targets for typed-or-linked `field-sort` entries); `searchColumns`
      from `field-filter` defaulting to the fields; `categories` joins
      the fields when categories are on; `page`/`pagination` only when
      items > page-size; `page-size: 0` → 50 (Q1's `|| 50`); nav markup;
      author text JSON-encoded with `<` escaped (no `</script>`
      breakout), nav id attribute-escaped.
- [x] Tests (`listing_render.rs`): default/grid containers end with
      nav + script after the no-matching placeholder; no nav when one
      page suffices; none for table, for custom (even with a `.list`),
      or under the `HtmlPreview` profile.
- [x] `project/listing/listjs.rs`; `binding::effective_fields` exposed
      (dropped its unused date-style parameter).
- [x] Emitted as raw-HTML blocks inside the container, gated on
      `PipelineProfile::{HtmlRender, RevealjsRender}`.

Implementation notes for bd-m6wglib4 (sort/filter UI): `list_options`
already carries the sort targets and `searchColumns`, and item
`data-listing-<f>-sort` attrs exist, so the follow-up is the
`_filter.ejs.md` port (controls + localized strings + Q1's
`sortableFieldData` ordering) and `sort-ui`/`filter-ui` accepting a
field list (forwarded to `field_sort`/`field_filter`). The sort
`<select>` calls `window['quarto-listings'][<container id>].sort(…)`,
so use `Listing::container_id()`.

### Phase 5 — Integration, docs, wrap-up

- [x] `listing_pipeline.rs` e2e
      (`paginated_listing_with_categories_carries_the_listjs_bootstrap`):
      repro-shaped project asserts container id/classes, item attrs,
      nav + script inside the container in order, script not
      HTML-escaped, List.js linked before the listing.
- [x] `listings.qmd`: "Pages and categories in the browser" (pagination,
      category filtering, page-size defaults, container id, what is not
      yet supported).
- [x] Re-rendered the investigation repro; jsdom sanity check
      (`listing-listjs-init-investigation/jsdom-check.cjs`, not a test):
      pagination, category filter and no-matching reveal all work, no
      script errors.
- [x] `cargo xtask verify` (full, with the WASM/hub build): all steps
      pass — 16063 Rust tests, all hub-client/ts-packages suites. (A
      `--skip-hub-build` run fails 122 hub-client wasm tests with
      "`wasm.<fn>` is not a function" when the local WASM bundle is older
      than the sources; that is a stale-bundle artifact, not this
      change.)

## Risks / tradeoffs

- **Visible changes for existing sites:** default/grid listings with
  more items than `page-size` now paginate (grid's default dropped from
  25 to 18); a lone listing's element id changes from `#listing-1` to
  `#listing-listing`, and an author's `::: {#id}` slot is renamed to
  `#listing-<id>` (Q1 behaviour). Authors who targeted the old ids in
  CSS or links need to update — called out in `listings.qmd`,
  `listing-templates.qmd`, `Q-12-4.qmd` and the retired `Q-12-25.qmd`.
- List.js `update()` detaches and re-appends item nodes on init.
  Anything that binds to listing items before `DOMContentLoaded` (none
  found in-tree) would lose its handlers.
- *Resolved:* the sort attrs survive the qmd re-parse (they are digits
  only by construction — unparseable values are skipped), checked by
  `item_metadata_attrs_land_on_the_list_children` and the e2e test.
- *Resolved:* grid attrs go on the outer `.g-col-1` (the `.list` child).
- **Known leftover (pre-existing, not a regression):** on a page whose
  only listings are custom (or table), `window['quarto-listings']` is
  never defined, so clicking a category in the sidebar makes
  `quarto-listing.js`\'s `filterListingCategory` throw a `TypeError` in
  the console (`Object.keys(undefined)`). It threw before this change
  too, and there is nothing to filter. Left alone rather than patching
  the vendored JS; revisit with bd-bl1e00r6 (tables) or bd-4dfdo8vi.
- **Multiple listings per page** each register their own
  `DOMContentLoaded`/`hashchange` handlers that call
  `quarto-listing-loaded()`, which re-binds `updated` handlers for every
  listing — a Q1 quirk reproduced faithfully (harmless duplicate
  handler work).

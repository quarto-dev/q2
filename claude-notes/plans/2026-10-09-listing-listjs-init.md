---
title: 'Listings: no List.js init script emitted — quarto-listing.js is inert (bd-nbv80e33)'
date: 2026-10-09
description: 'Q2 ships list.min.js and quarto-listing.js but never instantiates List.js, so pagination, category filtering and the no-matching reveal are dead; this scopes emitting the init script plus the item/page markup it depends on.'
status: in-progress  # Design aligned 2026-10-09; implementing
braid:
  strand: bd-nbv80e33
---

**Branch:** `braid/bd-nbv80e33-listing-listjs-init` (topic branch in the main checkout, based on `main` @ `ea72d68aa`)
**Design aligned 2026-10-09 — see § Decisions; implementation in progress.**

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
  listings' sort-ui/filter-ui/table-hover parity: sortable headers, row
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

- [ ] Tests: default/grid listing gets one `<script>` with
      `new List('listing-<id>', …)`, `valueNames` (fields as
      `listing-<f>`, `{data:['index']}`, `{data:['categories']}`, sort
      targets), `searchColumns`, and `page`/`pagination` only when
      items > page-size; pagination `<nav>` likewise; none for custom /
      table; none under the `HtmlPreview` profile; ids/fields are
      JS-string-escaped (no `</script>` breakout).
- [ ] New `project/listing/listjs.rs` generating the script + nav.
- [ ] Emit as raw-HTML blocks inside the container after the
      no-matching placeholder (`listing_render.rs::render_one`).

### Phase 5 — Integration, docs, wrap-up

- [ ] `listing_pipeline.rs` e2e: repro-shaped project (3 posts,
      page-size 2, categories) asserts script, nav, item attrs,
      container id; snapshots updated and reviewed.
- [ ] `listings.qmd`: interactivity section (pagination, category
      filtering, page-size defaults; not in hub-client preview yet).
- [ ] Re-render the investigation repro; `cargo xtask verify`.

## Risks / tradeoffs (draft)

- Turning pagination on changes how every existing Q2 listing with more
  than `page-size` items looks. That is intended for Q1 parity, but it is
  the most user-visible part of the change (see Q4).
- List.js `update()` detaches and re-appends item nodes on init. Anything
  that binds to listing items before `DOMContentLoaded` (none found
  in-tree) would lose its handlers.
- The item-wrapper attr splice goes through the qmd re-parse in
  `render_one`. `data-categories` b64 values are safe in qmd attribute
  syntax, but the sort attrs carry escaped date/number strings, so
  confirm they survive the re-parse.
- Grid's `.list` children are `.g-col-1` wrappers, so `data-*` attrs must
  go on that outer div, not on `.quarto-grid-item`.

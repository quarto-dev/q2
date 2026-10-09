---
title: 'Listings: no List.js init script emitted — quarto-listing.js is inert (bd-nbv80e33)'
date: 2026-10-09
description: 'Q2 ships list.min.js and quarto-listing.js but never instantiates List.js, so pagination, category filtering and the no-matching reveal are dead; this scopes emitting the init script plus the item/page markup it depends on.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-nbv80e33
---

**Branch:** `braid/bd-nbv80e33-listing-listjs-init` (topic branch in the main checkout, based on `main` @ `ea72d68aa`)
**Do not start implementation until the user gives the go-ahead.**

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
- Page-size defaults differ. Q1 uses table 30, grid 18 and default 50
  (`website-listing-read.ts:597`, with `|| 50` again in `templateJsScript`).
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

## Proposed phases (draft)

- **Phase 0: tests first.** Add unit tests in `listing_render.rs` that
  assert the init script is present, along with its container id and
  `valueNames`, `page` and `searchColumns` values. Add a binding/template
  test for item `data-index`/`data-categories` (b64). Add a smoke-all
  website fixture with `page-size` < items and categories, checking the
  `<nav class="listing-pagination">` and the `new List(` text. Optionally
  add a headless-browser check that clicking a category hides items. See
  Q6.
- **Phase 1: item metadata attrs.** Fix `metadata_attrs` to b64-encode
  categories the way Q1 does, add `listing-<field>-sort` attrs for
  date/number fields, and splice the attrs into the default and grid item
  wrappers (`::: {.quarto-post .image-right $metadata-attrs$}`, which
  needs to be a qmd attr form). Fix the stale doc comment.
- **Phase 2: init script emission.** Generate the per-listing script in
  `render_one` from `ResolvedListing`: id, fields, sort, filter,
  page-size and item count. Choose the emission site (Q2).
- **Phase 3: pagination markup.** Emit the `<nav … listing-pagination>`
  next to the no-matching div when `page_size < items.len()`. Reconcile
  the default page sizes (Q4).
- **Phase 4: sort/filter UI.** Port `_filter.ejs.md` to the default and
  grid listings, with localized strings. This could be split out (Q1).
- **Phase 5: custom listings.** Decide what custom templates get (Q3).
- **Phase 6: docs.** Update the L10 migration docs where they say
  interactivity is missing.

## Open design questions for the user

1. **Scope cut.** Should this strand cover only the bootstrap (init script,
   item `data-*` attrs and pagination nav, which together make categories,
   pagination and the no-matching reveal work), or also the sort/filter
   controls (`_filter.ejs.md`) for default and grid? I recommend bootstrap
   plus pagination here, and a sibling strand for the sort/filter UI.
   Table interactivity stays in bd-bl1e00r6 either way.
2. **Emission site.** Should the init script be a raw-HTML `<script>`
   block appended inside the listing container (next to the no-matching
   div, visible to Lua filters, travels with the AST), or a
   `header-includes` entry like Q1's `include-in-header`? I lean toward
   the in-container block for locality, since it still waits for
   `DOMContentLoaded`. Does hub-client preview execute inline scripts in
   rendered pages? If not, the header route has no advantage there
   either.
3. **Custom listings.** Q1 emits the init script for custom listings too.
   That works only if the author's template has a `.list` element and
   `data-*` attrs, and the script's `querySelector('#… .list')` guard
   bails out otherwise. Should Q2 do the same, always emitting the script
   and relying on the guard? Or should it emit only for built-in types,
   or gate it on a config key?
4. **Page-size defaults.** Should Q2 adopt Q1's defaults (default 50,
   grid 18, table 30) before pagination goes live? Otherwise every Q2 site
   with 26–50 items in a default listing starts paginating where Q1
   didn't.
5. **Container id parity.** Q1's DOM id is `listing-<id>` (default
   `#listing-listing`). Q2's is the bare listing id (`#listing-1`). The JS
   works with either as long as it is self-consistent. Should this strand
   match Q1's ids, since authors' CSS and anchor links may target
   `#listing-listing`, or should that be a separate strand?
6. **Verification depth.** Is a string-level smoke-all assertion
   (`new List(` present, ids consistent) enough, or do you want a
   browser-level check (Playwright in hub-client e2e, or a node+jsdom
   harness over the vendored JS) proving that clicking a category or a
   page link changes the visible items?

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

# Listing nits vs Q1: truncation ellipsis + hidden "No matching items" placeholder (bd-listing-ellipsis-no-matching-l963osy1)

**Date:** 2026-08-19
**Braid:** bd-listing-ellipsis-no-matching-l963osy1 (bug, p3, label `listings`)
**Checkout:** main checkout, branch `main` @ `87c0e21a8` (no worktree created — investigation only)
**Status:** Design aligned 2026-08-20 (all four questions answered by user; see
"Design decisions" below). Scope now **includes bd-pcmdb7qg** (explicit
`description:` truncation) as its own phase. Ready to implement — pending
choice of branch/worktree.

## Triage verdict

**Ready to design.** Both symptoms are confirmed at HEAD against the strand's
prebuilt repro; the fix sites are located and small. One scoping wrinkle was
discovered that the strand does not mention: q2 never emits the inline
List.js init script (`window["quarto-listings"][id] = new List(...)`), so the
vendored `quarto-listing.js` reveal logic is currently **inert** — emitting
the hidden div achieves markup parity but cannot yet be revealed by a filter,
because there is no functional filter. See design question 2.

## Issue context

Created 2026-08-19 (today) by Carlos; freshly verified against 0.24.0 = HEAD.
Two Q1-parity gaps in listings, both cosmetic, both on every listing page:

1. **Truncated auto-derived descriptions lose the trailing `…`.**
   `maybe_truncate` (`crates/quarto-core/src/project/listing/post_render_upgrade/reader.rs:187`)
   mirrors Q1's `truncateText(s, n, "space")` word-boundary cut but not its
   suffix. Q1's `trimAtSpace` (quarto-cli `src/core/text.ts:138-179`) does:
   cut at last space → strip one trailing `,` / `/` / `:` → append `…`.
2. **No hidden `<div class="listing-no-matching d-none">No matching items</div>`.**
   Q1 emits it from `projects/website/listing/_pagination.ejs.md`, appended
   after the rendered listing template for **all** listing types (including
   custom): `website-listing-template.ts` joins
   `[filterRendered, templateRendered, paginationRendered]`. The localized
   term is `listing-page-no-matches` (already present in q2's vendored
   `resources/language/_language*.yml` — all locales).

Real-world hit: ~14 Posit Connect docs cookbook index pages. Origin strand
`br-gny8y5v4` lives in the external connect-docs skein (not in this skein —
dependency graph here is empty).

## Dependency graph

Empty — no edges in this skein. Context strands referenced by description:

- **br-gny8y5v4** (connect-docs skein): origin, porting the Connect docs.
- **br-listing-default-no-derived-desc-ywc4zvu8** (connect-docs skein):
  *separate* bug — default-type listings emit no derived description at all
  (`item-default.template` guards with `$if(description)$`). Explicitly out
  of scope here, but the fix for nit 1 will make that gap more visible.

## What the code looks like today

All paths in the description are accurate at HEAD:

- `maybe_truncate` — `reader.rs:187-214`. Walks char indices, cuts at last
  space before `max`, `trim_end()`s, returns. No `…`, no `,`/`/`/`:` strip.
  Called only from `extract_first_para` (`reader.rs:149`), i.e. only the
  *derived*-description path (matches Q1: explicit `description:` is
  truncated by Q1 only in the List.js metadata record, not display... —
  see design question 3).
- Sibling truncation in the **feed** reader:
  `crates/quarto-core/src/project/listing/feed/reader_ext.rs:291`
  `maybe_truncate_visible` → `truncate_plain_at_word_boundary` — also no
  ellipsis. Q1's feed path truncates via `truncateNode` in
  `website-listing-shared.ts:507`, which calls the same ellipsis-appending
  `truncateText`. So the feed likely has the same nit (unverified in
  output). See design question 3.
- Container emission: `crates/quarto-core/src/transforms/listing_render.rs`
  (`render_one`, ~line 186). Renders the top-level template to markdown,
  re-parses with pampa, then either fills a user `::: {#id}` slot or appends
  a wrapper `Div` (id = listing id, class `quarto-listing`,
  `data-listing-rendered="1"`). **There is no pagination/filter/no-matching
  chrome anywhere** — grep for `pagination|no-matching` in the listing module
  finds only the `page-size` config plumbing.
- JS: `resources/listing/quarto-listing.js:91` (`toggleNoMatchingMessage`)
  queries `#<container-id> .listing-no-matching` — present and vendored. But
  it only runs from `list.on("updated")` inside `quarto-listing-loaded`,
  which iterates `window["quarto-listings"]` — **and q2 emits no init script
  populating that map** (Q1 emits an inline `new List('<id>', options)`
  script per listing; verified absent from the repro's q2 output, present in
  the Q1 output at `_site-q1/index.html:92-104`).
- Localization: `LanguageTerms::from_meta(&ast.meta).get("listing-page-no-matches")`
  is the established lookup pattern (see `toc_generate.rs:200-203`), and the
  term exists in every vendored `_language-*.yml`.
- SCSS: `.listing-no-matching` styling already vendored at
  `resources/scss/html/templates/quarto-listing.scss:257`.

### Repro (confirmed both symptoms at HEAD)

`/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/listing-ellipsis-no-matching/`
(external to this repo; prebuilt `_site` from 0.24.0 = HEAD, `_site-q1` from Q1):

- Ellipsis: `_site/custom.html` has `…listing page has to` (no marker);
  `_site-q1/index.html` has `…listing page has to…` (2 `…` total; q2 output
  has 0).
- Placeholder: `grep -c listing-no-matching` → 0 in both q2 pages, 1 in the
  Q1 page.

Not copied into `<slug>-investigation/` — it lives in the connect-docs repro
tree the strand already points at, and needs `posts/` + two Q1/Q2 renders to
be meaningful. (Phase-0 tests will build their own minimal fixtures in-tree.)

## Design decisions (2026-08-20, aligned with user)

1. **Placeholder as a qmd div**, not raw HTML: `::: {.listing-no-matching
   .d-none}` appended to the rendered template markdown before the re-parse
   (mirrors Q1's `[template, pagination].join`), so it lands inside the
   wrapper in both the appended-Div and explicit-slot paths — and stays
   visible to Lua filters / AST processing.
2. **Markup-parity only.** The List.js init gap stays in **bd-nbv80e33**.
3. **Feed ellipsis included — via one shared helper.** During scoping we
   found `truncate_plain_at_word_boundary` (`feed/reader_ext.rs:330`) is a
   verbatim copy of `maybe_truncate`'s body (`reader.rs:187`). Consolidate:
   one `truncate_text_at_space(s, max)` in the listing module (Q1
   `truncateText(s, n, "space")` parity: cut at last space ≤ max, strip one
   trailing `,`/`/`/`:`, append `…`), with the two existing wrappers keeping
   their disable semantics (`Some(0)`/`None`, `max == 0`) and the feed's
   visible-text projection. Kept local to the listing module (not
   quarto-util) — no non-listing consumer exists yet.
4. **bd-pcmdb7qg folded in as Phase 3.** Explicit `description:` truncation
   at `max-description-length`, Q1-parity. Site: **binding time**
   (`build_listing_context` / `binding.rs`), *not* `hydrate_item` — the item
   profile is per-document while `max-description-length` is per-listing
   (the same item can appear in multiple listings with different limits);
   Q1 likewise truncates when building per-listing template records
   (`website-listing-template.ts:130-140`).

## Work items

- **Phase 0 — Tests (TDD: write first, verify they fail).**
  - [ ] Unit tests for `truncate_text_at_space`: word-boundary cut + `…`
    suffix; trailing `,`/`/`/`:` stripped before the `…`; short strings
    untouched; no-space-in-window hard cut; multi-byte safety.
  - [ ] Wrapper-level tests: `maybe_truncate` `Some(0)`/`None` disable;
    feed `maybe_truncate_visible` `max == 0` disable + HTML projection,
    truncated output ends with `…`.
  - [ ] Integration tests (`crates/quarto-core/tests/integration/listing_pipeline.rs`):
    rendered listing page contains `listing-no-matching` + `d-none` with
    the localized/default "No matching items" text, for a built-in type
    and a custom-template listing; derived description ends with `…`.
  - [ ] Explicit-description test: item with long explicit `description:`
    renders truncated with `…` at the listing's `max-description-length`;
    a second listing with a different limit truncates differently
    (per-listing semantics); `max-description-length: 0` disables.
  - [ ] Check Q1's feed behavior for *explicit* descriptions before wiring
    Phase 3 into the feed path — if Q1 feeds don't truncate explicit
    descriptions, neither do we (parity, not invention).
- **Phase 1 — Shared truncation helper + ellipsis.**
  - [ ] Add `truncate_text_at_space` (listing module; likely `helpers.rs`).
  - [ ] Rewire `maybe_truncate` (reader.rs) and the feed's
    `maybe_truncate_visible`/`truncate_plain_at_word_boundary` onto it;
    delete the duplicate body.
- **Phase 2 — No-matching placeholder div.**
  - [ ] In `listing_render.rs::render_one`, append
    `::: {.listing-no-matching .d-none}` + localized term to the rendered
    template markdown before the re-parse, all listing types. Localize via
    `LanguageTerms::from_meta(&ast.meta).get("listing-page-no-matches")`,
    English fallback "No matching items" (toc-title fallback pattern).
- **Phase 3 — Explicit description truncation (bd-pcmdb7qg).**
  - [ ] Truncate explicit descriptions at binding time with the listing's
    `max_description_length` via the shared helper; derived-path
    placeholder markers must pass through untouched (Q1's `isPlaceHolder`
    guard equivalent).
- **Phase 4 — End-to-end verification.**
  - [ ] `cargo run --bin q2 -- render` on the connect-docs repro; inspect
    output for `…` and the placeholder div; record invocation + snippet
    here.
  - [ ] Snapshot audit: report any `.snap` changes per CLAUDE.md policy.
  - [ ] Full `cargo xtask verify` (quarto-core changes → WASM leg affected).
- **Phase 5 — Docs.** Likely none (behavior-parity fix); confirm
  `docs/` listing pages don't document the no-ellipsis behavior.

## Risks / tradeoffs (draft)

- The no-matching div goes through the listing markdown re-parse
  (`pampa::readers::qmd::read`) if emitted in the template/markdown path —
  raw HTML blocks inside the re-parsed markdown must survive; the existing
  image-placeholder machinery suggests raw HTML in listings is already
  exercised, but verify.
- Ellipsis change will touch listing-related snapshots (if any snapshot the
  derived descriptions) — audit and report per CLAUDE.md snapshot policy.
- `maybe_truncate` currently `trim_end()`s *all* trailing whitespace then we
  strip one of `,`/`/`/`:` — Q1's order is substring→trimSpace→trimEnd→`…`;
  match Q1's observable output on shared inputs rather than its exact
  internal order.
- Localization fallback: pages without a language catalog in meta must still
  get "No matching items" (mirror the toc-title fallback pattern).

# Listing nits vs Q1: truncation ellipsis + hidden "No matching items" placeholder (bd-listing-ellipsis-no-matching-l963osy1)

**Date:** 2026-08-19
**Braid:** bd-listing-ellipsis-no-matching-l963osy1 (bug, p3, label `listings`)
**Checkout:** main checkout, branch `main` @ `87c0e21a8` (no worktree created — investigation only)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

- **Phase 0 — Test plan (TDD).**
  - Unit tests for `maybe_truncate`: word-boundary cut + `…` suffix;
    trailing `,`/`/`/`:` stripped before the `…`; short strings untouched;
    `Some(0)`/`None` untouched; multi-byte safety.
  - Integration test (`crates/quarto-core/tests/integration/listing_pipeline.rs`)
    asserting the rendered listing page contains
    `class="listing-no-matching d-none"` with the localized/default text,
    for a built-in type and (if in scope) a custom-template listing; and that
    a derived description ends with `…`.
- **Phase 1 — Ellipsis in `maybe_truncate`** (reader.rs): after the cut +
  `trim_end`, strip one trailing `,`/`/`/`:`, append `…`. Decide feed-side
  (`maybe_truncate_visible`) per design question 3.
- **Phase 2 — Emit the hidden no-matching div**: in
  `listing_render.rs::render_one`, append the div after the rendered
  template markdown (mirroring Q1's `[template, pagination].join`) for all
  listing types, localized via `LanguageTerms` with English fallback
  "No matching items". Needs care with the markdown re-parse (emit as a
  raw-HTML block or a qmd div `::: {.listing-no-matching .d-none}` — decide
  in design).
- **Phase 3 — End-to-end verification**: `cargo run --bin q2 -- render` on a
  fixture (and optionally the connect-docs repro), inspect output, record in
  plan. Full workspace tests + `cargo xtask verify --skip-hub-build` minimum
  (listing changes live in `quarto-core` → full verify per CLAUDE.md).
- **Phase 4 — Docs** (probably none needed; behavior-parity fix).

## Open design questions for the user

1. **Placeholder placement.** Q1 appends the no-matching div as a *sibling
   right after* the template's list container, inside the outer
   `#<id>.quarto-listing` wrapper. In q2 the outer wrapper is either the
   appended Div or a user's explicit `::: {#id}` slot. Emitting it at the end
   of the re-parsed `parsed_blocks` (so it lands inside the wrapper in both
   paths) seems right — OK? And as raw HTML block vs. qmd div syntax?
2. **Scope vs. the inert JS.** q2 never emits the List.js init script, so
   nothing can currently reveal the placeholder (no functional
   filter/search/category filtering). The larger gap is now filed as
   **bd-nbv80e33** (discovered-from this strand; related to bd-bl1e00r6's
   table-specific sort/filter surface). Proposal: this strand stays
   markup-parity only (emit the div; cheap, matches Q1's static HTML) —
   confirm?
3. **Feed truncation.** Q1's feed path appends `…` too (same `truncateText`).
   Should `maybe_truncate_visible` in `feed/reader_ext.rs` get the same
   suffix in this strand, or be split out? (My read: include it — same
   one-line change shape, and leaving it diverges the two truncators.)
4. ~~**Explicit `description:` truncation.**~~ Resolved during investigation:
   q2 never truncates explicit descriptions (`item.rs:74-77` passes them
   through), while Q1 truncates at `max-description-length` with `…`
   (`website-listing-template.ts:134`). Filed as **bd-pcmdb7qg**
   (discovered-from this strand); out of scope here.

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

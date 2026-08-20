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

- **Phase 0 — Tests (TDD: write first, verify they fail).** ✅ 17 expected
  failures confirmed 2026-08-20 before any implementation.
  - [x] Unit battery on `maybe_truncate` (Q1-exact spec, 11 tests in
    `post_render_upgrade/reader.rs`): ellipsis, `,`/`/`/`:` strip, short
    unchanged, exactly-max truncated (Q1 quirk), hard cut, space-at-0 not
    a boundary, `None`/`Some(0)` disable, multi-byte, char-vs-UTF-16.
  - [x] Feed wrapper tests (`feed/reader_ext.rs`): word-boundary + `…`,
    tag-strip + comma-strip on truncation, `max == 0` disable (existing).
  - [x] Integration tests (`listing_pipeline.rs`, 5 new): placeholder div
    on default + custom listings, derived-description ellipsis on both,
    `lang: de` localization ("Keine Treffer"), explicit-description
    truncation, `max-description-length: 0` disable.
  - [x] Binding unit tests: explicit truncation at listing max; 0 disables.
  - [x] Q1 feed behavior for *explicit* descriptions checked: Q1 never
    truncates them in feeds (`truncateNode` only serves the derived
    placeholder fill) → Phase 3 does not touch the feed.
- **Phase 1 — Shared truncation helper + ellipsis.** ✅
  - [x] `truncate_text_at_space` added to `listing/helpers.rs` — an
    **exact port** of Q1 `truncateText(s, n, "space")`, including the
    strict-`<` fits-check and drop-one-before-space-search behavior
    (decision recorded below). Only documented divergence: Rust chars vs
    UTF-16 units.
  - [x] `maybe_truncate` (reader.rs) and the feed's
    `maybe_truncate_visible` rewired onto it; the verbatim-duplicate
    `truncate_plain_at_word_boundary` deleted. One pre-existing test
    (`substitute_description_truncates_to_max_from_marker`) updated to
    the new expected cut.
- **Phase 2 — No-matching placeholder div.** ✅
  - [x] `listing_render.rs::render_one` appends
    `::: {.listing-no-matching .d-none}` + localized
    `listing-page-no-matches` term (English fallback) to the rendered
    template markdown before the re-parse — all listing types.
- **Phase 3 — Explicit description truncation (bd-pcmdb7qg).** ✅
  - [x] `binding.rs::build_item_map` truncates explicit descriptions via
    the shared helper at the listing's `max_description_length`
    (0 disables). Derived-path markers unaffected (separate envelope
    machinery).
- **Discovered during Phase 2 verification: bd-yjsz6hdu.** ✅ (this
  strand's slice)
  - [x] `max-description-length: 40` (unquoted YAML integer) was silently
    ignored — `as_plain_text()` returns `None` for `Yaml::Integer`, so
    the 175 default always won. Fixed here via `parse_u32_scalar`
    (`as_int()` fallback) + regression test through the real YAML path.
    Sibling numeric keys (`page-size`, `max-items`, `grid-columns`)
    remain on the broken pattern → filed as **bd-yjsz6hdu**.
- **Phase 4 — End-to-end verification.**
  - [x] Rendered a pristine copy of the strand's repro through the real
    binary (2026-08-20):
    `cargo run --bin q2 -- render <scratch-copy-of-repro>` (sources from
    `~/repos/github/cscheid/q2-connect-docs/llms-info/repros/listing-ellipsis-no-matching/`,
    output inspected by hand). Both truncation endings are
    character-identical to the Q1 reference (`_site-q1/index.html`):
    `…listing page has to…` and `…guaranteeing that…` (2 ellipsis chars
    per page, matching Q1). Both listing pages now emit
    `<div class="listing-no-matching d-none"><p>No matching items</p></div>`
    (Q1: same div, text not wrapped in `<p>` — see decision above).
  - [x] Snapshot audit: **zero `.snap` changes** across the full
    workspace run.
  - [x] Full workspace tests: 12966/12966 passed. `cargo fmt --check`
    clean; `cargo clippy -p quarto-core` clean (one `map().unwrap_or()`
    nit fixed). Full `cargo xtask verify` (Rust + WASM + hub-client
    legs): **all steps passed** (2026-08-20).
- **Phase 5 — Docs.** ✅ No-op confirmed: nothing under `docs/` documents
  `max-description-length`, truncation, or the placeholder (the listings
  docs page is bd-2nb6i1qv's scope).

## Implementation decisions (recorded during execution)

- **Exact Q1 port, not a cleaned-up variant.** `truncate_text_at_space`
  reproduces Q1's `truncateText(s, n, "space")` char-for-char on BMP
  input, including two behaviors beyond the ellipsis that differ from
  q2's old cutter: (1) the fits-check is strict `<`, so an
  exactly-`max`-char string is truncated (Q1's `trimLength`); (2) the
  cut window is the first `max` chars *minus one*, cut at the last
  literal `' '` with index > 0 (old code cut at the last whitespace
  within `max`). Rationale: the strand's goal is Q1 parity; a variant
  that diverges on edge inputs would diff against Q1 forever.
- **Placeholder div text sits in a `<p>`** inside the div (qmd div →
  Para). Q1 puts the text directly in the div. Invisible while
  `d-none`; when revealed (bd-nbv80e33), `.listing-no-matching`'s
  centering/padding applies to the container either way — the inner
  `<p>` only adds its bottom margin.

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

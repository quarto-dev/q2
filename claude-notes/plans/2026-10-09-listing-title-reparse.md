---
title: "Listing item titles are flattened to plain text, then re-parsed as markdown (bd-8a9eum6p)"
date: 2026-10-09
---

# Listing item titles are flattened to plain text, then re-parsed as markdown (bd-8a9eum6p)

**Date:** 2026-10-09
**Braid:** bd-8a9eum6p
**Branch:** `braid/bd-8a9eum6p-listing-title-reparse` (topic branch in the main checkout, based on `main` @ `15bb0d54f`)
**Status:** Investigation. Design still needs to be agreed with the user. **Do not start implementation until the user gives the go-ahead.**

**Pre-flight:** `cargo xtask verify --skip-hub-build` is green at `15bb0d54f`. The first run hit a
flaky `marimo_engine_e2e::sc14` (a uv cache-rename race). That test passed on its own, and the full rerun exited 0.

## Triage verdict

**Ready to design.** The root cause is clear and confined to one place. pampa already has the
writer this fix needs (`write_inlines_fragment`). What is left to decide is scope: titles only or every
prose field, and whether the cached profile changes shape.

## Issue context

P1 bug, filed 2026-10-09 by Carlos while building the claude-notes plans listing.
Here is what happens. A listed page's front-matter `title:` is parsed as markdown. The listing
flattens it to plain text and drops it into the generated listing markdown, then parses that
markdown again. This causes two failures:

1. **Formatting is lost.** `Plan for `q2 preview` and *emph*` renders on its own page with
   `<code>` and `<em>`, but the listing shows plain text.
2. **The whole listing disappears.** If the *plain text* means something in markdown, the
   re-parse fails: a leading `_` opens emphasis that never closes, `<anonymous>` reads as raw
   HTML, and backticks open code spans. The result is a Q-12-10 warning (an error under
   `--strict`), and the listing is skipped entirely. 27 of the 1050 claude-notes plan titles
   hit this.

The strand's first theory, that `*` is re-escaped but `_` is not, was wrong. The comment of
2026-10-09 corrects it: there is no escaping at all. A lone `*`, `~` or `@` survives only because
nothing in markdown pairs with it.

## Dependency graph

- **discovered-from** bd-fvcip3t5 (claude-notes plans listing page, in_progress). That branch
  backfilled `title:` front matter on all 1050 plans. Its listing fails strict render
  with 1 error because of this bug. It is **blocked** on deciding between fixing this and working around it.
  The workaround already used in claude-notes is a code span around the identifier, but code
  spans are exactly what this bug flattens, so the workaround depends on which titles happen to be affected.
- **related** bd-mlmkev01 (listings with 90+ items dropped, in_progress, has its own plan
  `2026-10-09-listing-nesting-limit.md`). Same blast radius: one bad re-parse drops the
  whole listing. Its cause is different (template separators), but it touches the
  same templates. Merge order matters only if both edit `item-*.template`; this fix probably won't.
- **related** bd-61cd (Listings feature epic, open), the parent area.

## What the code looks like today

The data flow, all at HEAD `15bb0d54f`:

1. `crates/quarto-core/src/document_profile.rs:990`: `title: plain_text_field(meta, "title")`.
   `plain_text_field` calls `ConfigValue::as_plain_text`. For `PandocInlines`, that runs
   `inlines_to_plain_text` (`crates/quarto-pandoc-types/src/config_value.rs:779`). **This is
   where the information is lost.** `subtitle` and `description` are flattened the same way (lines 991–992), and so are
   the `listing-item:` overrides in `ListingItemInfo::from_map` (around line 401).
   Inline `contents:` records go through the same `from_map` (`project/listing/record.rs`).
2. `project/listing/item.rs:173` hydrates `ListingItem.title: String` from
   `listing_item.title`, then `profile.title`, then the filename stem.
3. `project/listing/binding.rs:271` puts that string, unescaped, into the template binding
   as `title`. `binding.rs:608`, through `table_row`, does the same for table cells, where only `|` and
   newlines are escaped (`escape_table_cell`).
4. The built-in templates (`project/listing/templates/item-{default,grid}.template`) interpolate
   `$title$` / `$subtitle$` / `$description$` straight into markdown, for example
   `### [$title$]($path$){...}`.
5. `transforms/listing_render.rs` re-parses the output with `pampa::readers::qmd::read`. On any
   diagnostic it emits Q-12-10 and skips the listing (around line 265).

Other consumers of the same plain-text title, which **must stay plain text**: sort
(`sort.rs:104`), filter (`filter.rs:86`), feed/RSS (`feed/binding.rs:340`, which is XML-escaped),
and the feed's empty-title check (`feed/stage.rs:318`).

A suitable writer already exists: `pampa::writers::qmd::write_inlines_fragment`
(`crates/pampa/src/writers/qmd.rs:3349`). It writes inlines back as markdown that can sit
inside a line, escapes `_` / `*` and the rest, and was built for the same round-trip
problem in diagnostics (bd-q249). `write_inlines` (used by `q2 get-config`) is the variant
that assumes it starts a line.

**Q1 behavior.** Q1 takes `title` as the raw markdown string from YAML
(`project/project-index.ts:104`, then `website-listing-read.ts:1188`). It puts that string into the EJS
markdown and lets Pandoc parse it once. Code spans and emphasis therefore survive in Q1 listings.

**Repro, confirmed at HEAD `15bb0d54f`:** `claude-notes/plans/listing-title-reparse-investigation/repro/`
(5 pages, a table listing and a default listing; see `findings.md` alongside it for the output).
- With all 5 pages, **both** listings are dropped: `Q-12-10 … Unclosed Underscore Emphasis. Listing skipped.`
- Without the two `_scope` pages, both listings render, but:
  - `Plan for `q2 preview` and *emph*` becomes `Plan for q2 preview and emph` (formatting lost);
  - `About `<anonymous>` frames` becomes `<a …>About <anonymous> frames</a>`. **The plain text is
    re-read as a raw HTML element.** The browser hides it, so the title shows as "About  frames", and
    Q-12-10 still warns ("HTML element converted to raw HTML"). This is a third symptom the strand
    does not mention: page-title text can inject HTML into the listing page.

## Proposed phases (draft)

- **Phase 0: failing tests.** A unit test in `binding.rs` that builds an item whose title is
  `_scope` / contains a code span and asserts that the bound markdown re-parses to the same inlines. An
  integration test (`crates/quarto-core/tests/integration/…`) that renders the repro project and
  asserts there is no Q-12-10, that `<code>_scope</code>` appears inside the listing, and that both
  listings are present.
- **Phase 1: keep the inlines.** Carry the parsed inlines (or a markdown form of them) from the
  profile to `ListingItem` next to the plain-text `title`. See Q1 below for the shape.
- **Phase 2: bind markdown, not text.** In `build_item_map` / `table_row`, bind `title`
  (and possibly subtitle and description) as `write_inlines_fragment(inlines)`. Leave sort,
  filter and feed on the plain-text projection. The filename-stem fallback and plain-string
  sources need escaping too: wrap them in `Str` and run them through the same writer.
- **Phase 3: harden the table path.** Check that `escape_table_cell` still applies cleanly to
  writer output, for example a code span that contains `|`.
- **Phase 4: unblock bd-fvcip3t5.** Re-render the claude-notes plans listing under `--strict`.
  Drop the code-span workaround if it is no longer needed.

## Open design questions for the user

1. **Profile shape.** Should titles be stored as `Inlines` on `DocumentProfile` /
   `ListingItemInfo` (following v11's TocEntry change, which bumps `DOCUMENT_PROFILE_VERSION` to 14
   and forces cached profiles to regenerate)? Or should a pre-rendered markdown string be stored next to
   the existing plain-text field (smaller diff, but a derived value in the cache)? I lean toward `Inlines`,
   with plain text computed at the consumer, as the TocEntry precedent did.
2. **Scope.** Fix only `title`, or also `subtitle` and `description`? Both go through the
   same flatten-then-re-parse path and break the same way. `description` is the larger
   change because it is block-ish and is truncated by `max-description-length` in plain text.
   Proposal: title and subtitle now, description as a follow-up strand.
3. **Table cells.** Should a table listing's title cell keep formatting (code spans and so on), or only stop
   breaking? Q1 keeps formatting in both layouts, so I propose formatting in both.
4. **Defense in depth.** Separately from this fix, should one item that fails to re-parse
   still remove the *whole* listing? A per-item fallback (re-parse each item, then
   escape the plain text for any item that fails) would also cover bd-mlmkev01's class of
   failure. Should that be in scope here, or a separate strand?

## Risks / tradeoffs (draft)

- Bumping the profile version invalidates every cached profile. That is cheap, but it is a cold-start cost for
  large projects.
- `write_inlines_fragment` output that contains a `Link` would nest a link inside the
  `[$title$]($path$)` link. Q1 has the same issue (Pandoc then emits nested `<a>`). It needs a decision:
  probably flatten `Link` to its content in the title projection.
- Sidebar, breadcrumbs and navigation also use the plain-text `profile.title`
  (`sidebar_auto.rs`, `navigation_enrich.rs`). They do not appear to re-parse, so they lose
  formatting but do not break. That is out of scope here and could be filed as a related strand if wanted.

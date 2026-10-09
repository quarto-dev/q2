---
title: "Listing item titles are flattened to plain text, then re-parsed as markdown (bd-8a9eum6p)"
date: 2026-10-09
---

# Listing item titles are flattened to plain text, then re-parsed as markdown (bd-8a9eum6p)

**Date:** 2026-10-09
**Braid:** bd-8a9eum6p
**Branch:** `braid/bd-8a9eum6p-listing-title-reparse` (topic branch in the main checkout, based on `main` @ `15bb0d54f`)
**Status:** Design agreed with the user on 2026-10-09 (see Decisions). Two smaller points (D5, D6) are my defaults and still open to change. **Do not start implementation until the user gives the go-ahead.**

**Pre-flight:** `cargo xtask verify --skip-hub-build` is green at `15bb0d54f`. The first run hit a
flaky `marimo_engine_e2e::sc14` (a uv cache-rename race). That test passed on its own, and the full rerun exited 0.

## Triage verdict

**Ready to implement once the user gives the go-ahead.** The root cause is clear and confined to one place, pampa already has the
writer this fix needs (`write_inlines_fragment`), and scope was settled with the user on 2026-10-09.

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

## Decisions (2026-10-09, with the user)

- **D1. Profile shape: `Inlines`.** `DocumentProfile.{title,subtitle,description}` and
  `ListingItemInfo.{title,subtitle,description}` become `Option<Inlines>`, following the v11
  `TocEntry::title` precedent. `DOCUMENT_PROFILE_VERSION` goes from 13 to 14, and cached profiles regenerate.
  Consumers that need plain text (sort, filter, feed/RSS, the feed's empty-title check, sidebar,
  navigation, llms, search) compute it with `pampa::writers::plaintext::inlines_to_string`.
- **D2. Scope: title, subtitle *and* description.** A description is a single `Inlines`. Authors
  should write "about a paragraph". (A separate thread already wants description carried in
  metadata so listings work better.)
- **D3. Formatting is kept in every layout,** default, grid and table cells alike, as in Q1.
- **D4. A failed re-parse is an error, not a warning.** The `Err` branch of the re-parse
  (`listing_render.rs` around line 260, "… failed … Listing skipped.") emits Q-12-10 with
  error severity, so the render reports it and does not silently ship a page without the
  listing. There is precedent for transform-level errors: crossref's Q-15-1, which
  `ProjectRenderSummary` counts as an error.

My defaults, not yet confirmed:

- **D5. Multi-block descriptions.** `description: |` with two paragraphs parses to
  `PandocBlocks`, and today `as_plain_text` returns `None` for that, so the description is
  probably dropped silently (to confirm in Phase 0). Proposal: flatten the `Para`/`Plain` blocks into one
  `Inlines` joined by `Space`, and emit a *warning* that suggests a single paragraph. Other
  block types (lists, code blocks) keep their inline text only.
- **D6. Non-fatal re-parse diagnostics stay warnings.** The `Ok` branch ("produced N
  diagnostic(s)") stays a warning, because custom templates legitimately write raw HTML and the
  "HTML element converted to raw HTML" diagnostic would then fail their render. After D1–D3,
  item *data* can no longer trigger it. Only template text can.
- **D7. Truncation becomes inline-aware.** `max-description-length` currently truncates a
  `String` (`helpers::truncate_text_at_space`). It needs an `Inlines` version that counts the plain-text
  length, cuts at a word boundary inside a `Str`, drops the inlines after the cut (and closes nothing,
  because the result is a tree, not text), and appends `…`. Derived descriptions (filled in after render from the
  HTML) are unaffected.

## Phases

- **Phase 0: failing tests first.**
  - Unit (`binding.rs`): an item whose title is `[Str "_scope"]` or contains `Code "_scope"` /
    `Code "<anonymous>"` / `Emph`. Assert that the bound `title` markdown, run through
    `pampa::readers::qmd::read`, gives back the same inlines. Do the same for a table cell that contains a `|`
    inside a code span.
  - Unit (`document_profile.rs`): the profile keeps `title` as inlines (code span intact).
    Probe the multi-paragraph description (D5) and record what happens today.
  - Integration (`crates/quarto-core/tests/integration/listing_title_markup.rs`, registered in
    `main.rs`): render the repro project
    (`claude-notes/plans/listing-title-reparse-investigation/repro/`, copied into a fixture).
    Assert that both listings are present, that `<code>_scope</code>` and
    `<code>&lt;anonymous&gt;</code>` appear inside listing titles and table cells, that `<em>emph</em>`
    survives, and that there is no Q-12-10.
  - Integration: a custom template that cannot be re-parsed gives Q-12-10 with **error** kind (D4).
- **Phase 1: profile.** Switch the three fields to `Option<Inlines>` on `DocumentProfile` and
  `ListingItemInfo` (`from_map`, and inline `contents:` records via `record.rs`), add a
  `ConfigValue → Inlines` helper (a `Str` from a plain YAML string, `PandocInlines` as-is,
  `PandocBlocks` per D5), bump to v14, and update the version doc comment. Move the plain-text consumers
  to `inlines_to_string`.
- **Phase 2: listing item and binding.** `ListingItem.{title,subtitle,description}` become `Inlines`.
  The filename-stem fallback is `vec![Str(stem)]`, so its `_` gets escaped automatically.
  `build_item_map` and `table_row` bind `write_inlines_fragment(..)` output. Flatten `Link` to
  its content first (risk 2), because the templates wrap the title in a link. `escape_table_cell`
  still runs over the writer output. Check that the writer escapes `|` inside code spans,
  or handle it.
- **Phase 3: truncation (D7)** and the multi-block warning (D5), with a new code in
  `error_catalog.json`.
- **Phase 4: Q-12-10 to error (D4).** Split `push_diag` into warning and error variants, and update the
  catalog entry (title and message) to say that a failure skips the listing *and* fails the render.
  Coordinate with bd-mlmkev01: until it lands, a listing with 90 or more items becomes a hard error rather
  than a vanishing listing. That is arguably the point, but land mlmkev01 first or alongside.
- **Phase 5: unblock bd-fvcip3t5.** Render the claude-notes plans listing with `--strict`
  on that branch, and drop any code-span workarounds that are no longer needed.
- **Phase 6: docs.** Q-12-10 docs page or catalog text, and listing docs that say descriptions
  should be one paragraph of markdown.

## Risks / tradeoffs (draft)

- Bumping the profile version invalidates every cached profile. That is cheap, but it is a cold-start cost for
  large projects.
- `write_inlines_fragment` output that contains a `Link` would nest a link inside the
  `[$title$]($path$)` link. Q1 has the same issue (Pandoc then emits nested `<a>`). It needs a decision:
  probably flatten `Link` to its content in the title projection.
- Sidebar, breadcrumbs and navigation also use the plain-text `profile.title`
  (`sidebar_auto.rs`, `navigation_enrich.rs`). They do not appear to re-parse, so they lose
  formatting but do not break. That is out of scope here and could be filed as a related strand if wanted.

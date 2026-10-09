---
title: "Listing item titles are flattened to plain text, then re-parsed as markdown (bd-8a9eum6p)"
date: 2026-10-09
---

# Listing item titles are flattened to plain text, then re-parsed as markdown (bd-8a9eum6p)

**Date:** 2026-10-09
**Braid:** bd-8a9eum6p
**Branch:** `braid/bd-8a9eum6p-listing-title-reparse` (topic branch in the main checkout, based on `main` @ `15bb0d54f`)
**Status:** Design agreed (D1–D7) on 2026-10-09; implementation in progress. Progress is tracked in the Phases checklists.

**Pre-flight:** `cargo xtask verify --skip-hub-build` is green at `15bb0d54f`. The first run hit a
flaky `marimo_engine_e2e::sc14` (a uv cache-rename race). That test passed on its own, and the full rerun exited 0.

## Triage verdict

**Ready; implementation started 2026-10-09.** The root cause is clear and confined to one place, pampa already has the
writer this fix needs (`write_inlines_fragment`), and scope was settled with the user on 2026-10-09.

## Issue context

P1 bug, filed 2026-10-09 by Carlos while building the claude-notes plans listing.
Here is what happens. A listed page's front-matter `title:` is parsed as markdown. The listing
flattens it to plain text and drops it into the generated listing markdown, then parses that
markdown again. This causes two failures:

1. **Formatting is lost.** ``Plan for `q2 preview` and *emph*`` renders on its own page with
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
  - ``Plan for `q2 preview` and *emph*`` becomes `Plan for q2 preview and emph` (formatting lost);
  - ``About `<anonymous>` frames`` becomes `<a …>About <anonymous> frames</a>`. **The plain text is
    re-read as a raw HTML element.** The browser hides it, so the title shows as "About  frames", and
    Q-12-10 still warns ("HTML element converted to raw HTML"). This is a third symptom the strand
    does not mention: page-title text can inject HTML into the listing page.

## Decisions (2026-10-09, with the user)

- **D1. Profile shape: `Inlines`.** `DocumentProfile.{title,subtitle,description}` and
  `ListingItemInfo.{title,subtitle,description}` become `Option<Inlines>`, following the v11
  `TocEntry::title` precedent. `DOCUMENT_PROFILE_VERSION` goes from 13 to 14, and cached profiles regenerate.
  Consumers that need plain text (sort, filter, feed/RSS, the feed's empty-title check, sidebar,
  navigation, llms, search) compute it with `quarto_pandoc_types::inlines_to_plain_text`. *As built:*
  this is the projection `ConfigValue::as_plain_text` applied before, not the plaintext writer the
  draft named, so their output is byte-for-byte unchanged. Accessors: `DocumentProfile::title_text()` and so on,
  and `ListingItem::title_text()` and so on.
- **D2. Scope: title, subtitle *and* description.** A description is a single `Inlines`. Authors
  should write "about a paragraph". (A separate thread already wants description carried in
  metadata so listings work better.)
- **D3. Formatting is kept in every layout,** default, grid and table cells alike, as in Q1.
- **D4. A failed re-parse is an error, not a warning.** The `Err` branch of the re-parse
  (`listing_render.rs` around line 260, "… failed … Listing skipped.") emits Q-12-10 with
  error severity, so the render reports it and does not silently ship a page without the
  listing. There is precedent for transform-level errors: crossref's Q-15-1, which
  `ProjectRenderSummary` counts as an error.

Confirmed by the user on 2026-10-09 (proposed by me):

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
  length, cuts at a word boundary inside a `Str`, drops the inlines after the cut (nothing needs closing,
  because the result is a tree, not text), and appends `…`. Derived descriptions (filled in after render from the
  HTML) are unaffected. The cut may not match Q1's character count exactly; the user accepted that.

Added during implementation (2026-10-09):

- **D8. `-html` twins for raw-HTML templates.** `$it.title$` / `$it.subtitle$` / `$it.description$` are now
  markdown. A custom template that writes them inside a raw HTML block, as our own
  `ejs-listing-port` worked example does, would show the markdown source (`Fix \_scope`). The binding therefore
  also provides `title-html` / `subtitle-html` / `description-html`, rendered by the HTML inline
  writer. This follows the existing `image-html` / `category-html` convention. The docs and skill say which
  to use where.
- **D5 delivery.** The profile is cached, and its spans belong to the listed file's source context,
  not the host's. So the profile records only *which* keys were flattened (`flattened_prose`), and the
  listing reports Q-12-26 on every host render, naming the document (no span). The report follows
  `hydrate_item` precedence: a `listing-item:` or inline-record value that shadows a flattened one
  draws no warning. Inline records warn directly in `parse_record`, *with* a span. Lists flatten item by
  item (`prose_runs`), because `blocks_to_inlines` (the pandoc.utils function) glues list items together with no separator.
- **Prose is written on one line.** Soft and hard breaks become spaces, because titles sit in ATX headings
  and table cells. Links are unwrapped and notes dropped (`strip_links_and_notes`, shared with
  the TOC), because every built-in template wraps the title in the item's link.

## Phases

### Phase 0: failing tests first
- [x] Probe today's behavior: a multi-paragraph `description: |` (D5), and the qmd writer's
      handling of `|` inside a code span. Record both in the investigation dir.
- [x] *(moved to Phase 1; needs the new types to compile)* Unit (`document_profile.rs`): the profile keeps `title` / `subtitle` / `description` as inlines
      (code span intact).
- [x] *(moved to Phase 2; needs the new types to compile)* Unit (`binding.rs`): items whose title is `[Str "_scope"]`, or contains `Code "_scope"`,
      `Code "<anonymous>"` or `Emph`. Re-parsing the bound `title` markdown with
      `pampa::readers::qmd::read` gives back the same inlines. Same for a table-row cell, including a `|` inside
      a code span.
- [x] Integration (`tests/integration/listing_title_markup.rs`): the repro project. Both
      listings present; `<code>_scope</code>`, `<code>&lt;anonymous&gt;</code>` and `<em>emph</em>` appear in
      listing titles (default) and table cells; subtitle and description markup survives; no Q-12-10.
- [x] Integration: a custom template that fails to re-parse gives Q-12-10 with **error** kind (D4).
- [x] Confirm the new tests fail for the right reason at HEAD. All 3 fail: Q-12-10 present; the
      multi-paragraph description cell is empty; Q-12-10 has warning kind. The multi-paragraph test pins
      the new warning code as **Q-12-26** (the next free Q-12 code on `main`; check for collisions with
      bd-mlmkev01 before merging).

### Phase 1: profile (D1, D2, D5)
- [x] `ConfigValue → Option<Inlines>` helper: a plain YAML string becomes `Str`s; `PandocInlines` is
      used as-is; `PandocBlocks` is flattened per D5 (with a flag so the caller can warn).
- [x] `DocumentProfile.{title,subtitle,description}` and `ListingItemInfo.{title,subtitle,description}`
      become `Option<Inlines>`.
- [x] `DOCUMENT_PROFILE_VERSION` 13 → 14, plus a doc-comment entry.
- [x] Move every plain-text consumer to `inlines_to_string` (sidebar, navigation, llms, search,
      index, book, sort, filter, feed, …).

### Phase 2: listing item and binding (D3)
- [x] `ListingItem.{title,subtitle,description}` become `Inlines`; the filename-stem fallback is `Str`.
- [x] Inline `contents:` records (`record.rs`) produce inlines.
- [x] `build_item_map` / `table_row` bind `write_inlines_fragment` output, with links flattened in
      titles; table cells stay safe for `|` and newlines.

### Phase 3: truncation (D7) and multi-block warning (D5)
- [x] Inline-aware truncation for `max-description-length`.
- [x] Warning (new catalog code) for a multi-block description.

### Phase 4: a failed Q-12-10 re-parse becomes an error (D4, D6)
- [x] The `Err` branch emits an error; the `Ok`-with-diagnostics branch stays a warning.
- [x] Update the catalog entry text.
- [x] Note the bd-mlmkev01 ordering on both strands (braid comments, 2026-10-09).

### Phase 5: verification and unblocking
- [x] `cargo xtask verify` green (full, including the hub build), 2026-10-09, after `cargo fmt`.
- [x] Repro project renders with both listings and formatting intact (`q2 render`, no
      diagnostics). A broken custom template now prints `Error [Q-12-10]` and exits 1.
- [x] *Simulated* the bd-fvcip3t5 plans listing (that branch is not in this checkout):
      `listing-title-reparse-investigation/plans-listing-sim.py` gives each of the 1044 top-level plans a
      title from its H1 and lists them all in a table. `q2 render --strict`: 1045 of 1045 files, exit 0,
      no diagnostics; 1044 rows, 191 titles keep `<code>` spans, none leak raw HTML.
- [ ] On the bd-fvcip3t5 branch itself, after merging this: re-render with `--strict` and drop any
      code-span workarounds (owner of that branch).

### Phase 6: docs
- [x] Catalog/docs text for Q-12-10 and the new warning; listing docs say a description should be
      one paragraph.
- [x] `docs/errors/listing/Q-12-10.qmd` rewritten (warning vs error; item data no longer a
      trigger); new `Q-12-26.qmd`, registered in `docs/_quarto.yml`; `listings.qmd` gains
      "Titles and descriptions are markdown" and the `-html` twins; `listing-templates.qmd` mapping
      row; `ejs-listing-port` skill updated for D8.

## Follow-ups found (not in scope)

- **bd-listing-description-precedence-x4bh6w3m** (already filed, open): the default and grid layouts
  replace a document's *explicit* description with the derived first paragraph, because placeholders
  are emitted for every document item. Q1 emits them only when there is no description or abstract.
  The integration test checks document descriptions in the table and uses an inline record for
  the template path because of this.
- Other string fields interpolated into listing markdown (`author`, `date`, `categories` in
  templates, custom `extra` fields, non-prose table cells) are still unescaped plain text: the same
  class of bug, much rarer in practice (an author named `_x`).
- Sidebar, breadcrumbs, navbar and `llms.txt` still show plain-text titles (formatting lost, nothing
  breaks). Unchanged by this work.
- A document listed twice on one host page warns Q-12-26 once per listing.

## Risks / tradeoffs (draft)

- Bumping the profile version invalidates every cached profile. That is cheap, but it is a cold-start cost for
  large projects.
- ~~Nested links~~: resolved, since links are unwrapped before writing (see "Prose is written on one line").
- Sidebar, breadcrumbs and navigation also use the plain-text `profile.title`
  (`sidebar_auto.rs`, `navigation_enrich.rs`). They do not appear to re-parse, so they lose
  formatting but do not break. That is out of scope here and could be filed as a related strand if wanted.

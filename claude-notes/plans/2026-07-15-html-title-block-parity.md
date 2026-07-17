# HTML Title Block Parity with Quarto 1 (bd-gx9cic8z)

**Status: DESIGN SETTLED (2026-07-15) — awaiting explicit approval to start
execution.** All open questions resolved with Carlos; see "Design decisions"
below. Q5 (banner `<style>` vs SCSS) got a written clarification, recorded
inline.

Braid epic: `bd-gx9cic8z`. Related bug filed during study: `bd-8v34zny5`
(structured authors render as `truetrue`).

## Overview

Bring Q2's HTML title block to parity with Quarto 1's. Q1's title block is a
rich feature: structured authors/affiliations with ORCID/email/URL decoration,
a localized metadata grid (published/modified/DOI/abstract/keywords), category
chips, three named styles (`default`, `plain`, `none`, plus `manuscript`), and
a banner mode with theme-derived or explicit background color/image.

Guiding principle (from the kickoff discussion): we do **not** need
byte-for-byte output equivalence, but replicating Q1's DOM structure where
practical means the already-ported SCSS and existing third-party extensions
keep working with minimal changes. The recommendation throughout this plan is:
**match Q1's element nesting and class names exactly for the title block
proper; do not chase Q1's template whitespace or attribute ordering.**

## Current state (studied 2026-07-15)

### How Q1 does it

Everything under `external-sources/quarto-cli`:

- **Templates**: Pandoc template partials in
  `src/resources/formats/html/templates/`: `title-block.html` (default),
  `banner/title-block.html`, `manuscript/title-block.html`, shared
  `title-metadata.html` metadata grid, and `_title-meta-author.html` (one
  author: url link, degrees, `quarto-title-author-email` envelope icon,
  `quarto-title-author-orcid` badge — a base64 PNG linking to orcid.org).
- **Author normalization**: the Lua filter
  `src/resources/filters/modules/authors.lua` normalizes `author`/`authors`
  into `authors`, `affiliations`, **`by-author`**, **`by-affiliation`** (the
  shapes the templates iterate), and `funding`. Full schema: structured
  `name` (given/family/literal/particles), `degrees`, `orcid`, `email`,
  `phone`, `url`, `note`, `roles` (CRediT), attribute flags
  (`corresponding`, `equal-contributor`, `deceased`), affiliations with
  `department/group/address/city/region/country/postal-code/url/isni/ringgold/ror`,
  and `ref:`/`id` cross-referencing with a top-level `affiliations:` block.
- **Labels**: `computeLabels()` in authors.lua writes a `labels` meta table
  from language keys (`title-block-author-single`/`-plural`,
  `title-block-published`, `title-block-modified`, `title-block-keywords`,
  `section-title-abstract`, …) in `src/resources/language/_language.yml`,
  overridable per-document via `author-title`, `affiliation-title`,
  `published-title`, `modified-title`, `doi-title`, `abstract-title`,
  `description-title`.
- **TS orchestration**: `src/format/html/format-html-title.ts` —
  `documentTitlePartial()` picks the partial set and template params
  (`title-block-categories`, `banner-header-class: toc-left`);
  `documentTitleScssLayer()` adds `templates/title-block.scss` unless style
  is `plain`/`none`; `documentTitleMetadata()` forces `date-format: long`
  default when styled; `documentTitleIncludeInHeader()` emits an inline
  `<style>` for explicit banner colors / background images;
  `processDocumentTitle()` is a **DOM postprocessor** that relocates the
  header above `#quarto-content` in banner/manuscript mode and adds
  `quarto-banner-title-block` to `main.content` and `quarto-banner` to
  `#quarto-header`.
- **Options** (`src/resources/schema/document-layout.yml`):
  `title-block-style` (`default|plain|manuscript|none`), `title-block-banner`
  (bool | CSS color | image path), `title-block-banner-color`
  (`body|body-bg|<color>`), `title-block-categories` (bool, default true).
- **Banner colors in SCSS**: `templates/title-block.scss` functions
  `bannerColor()` ($title-banner-color → $navbar-fg → $body-bg) and
  `bannerBg()` ($title-banner-bg → $navbar-bg → $body-color); explicit
  user-specified colors/images arrive via the include-in-header style block
  instead.
- **Not in the title block**: `license`, `copyright`, `citation` render in
  the **appendix** (`format-html-appendix.ts`); `funding` is normalized but
  surfaced mainly in JATS/manuscript.

### What Q2 has today

- **The full-mode title block is inline literal markup** in
  `FULL_HTML_TEMPLATE`, `crates/quarto-core/src/template.rs:216-245`:
  `header#title-block-header.quarto-title-block.default > div.quarto-title >
  h1.title / p.subtitle`, then a `div.quarto-title-meta` with
  author/date entries and an abstract div. Only `title`, `subtitle`,
  `abstract` render rich inline content (`RICH_TITLE_BLOCK_FIELDS`,
  `template.rs:648`); `author`/`date` flatten to plain text
  (strand bd-5706gcrq), which is what produces the `truetrue` bug
  (bd-8v34zny5) for structured authors.
- **`TitleBlockTransform`** (`crates/quarto-core/src/transforms/title_block.rs`)
  only injects an `<h1>` in minimal mode / non-HTML formats; in full mode the
  template is authoritative. Phase: Normalization; spliced in
  `pipeline.rs` after `MetadataNormalizeTransform`.
- **`MetadataNormalizeTransform`** only derives `pagetitle` today; its module
  doc anticipates more derived fields.
- **`DocumentProfile`** (v6) has `authors: Vec<String>` — flat names only;
  `document_profile.rs:742-745` explicitly defers a structured author model
  to "a separate epic". **This is that epic.**
- **SCSS is ahead of the markup**: `resources/scss/html/templates/title-block.scss`
  is already the Q1 port — it styles `.quarto-title-banner`,
  `main.quarto-banner-title-block`, `.quarto-categories`/`.quarto-category`,
  `.quarto-title-author-orcid/-email`, `.abstract .block-title`, and carries
  `bannerColor()/bannerBg()/bannerDim()` and `$title-banner-*` variables.
  Nothing emits that markup yet, and some of what Q2 does emit doesn't match
  the ported selectors (see diff table).
- **Doctemplate supports partials** (`$title-block.html()$`-style), with
  resolvers already wired for custom `template-partials`
  (`apply_template.rs:379-406`). The built-in template just doesn't use them.
- **q2-preview renders the title block in React**
  (`hub-client/.../PreviewTitleBlock.tsx`, plan 2D), mirroring the current
  Rust markup. Any markup change here must be propagated there.
- **Appendix already exists** in Q2 (Reuse/Copyright/Citation sections render
  today via `AppendixStructureTransform`), so license/copyright/citation are
  **out of scope** for this epic except where they intersect (e.g. CC link
  detection is an appendix concern).

### Empirical DOM diff (fixtures rendered 2026-07-15)

Fixtures: a simple doc (title/subtitle/author/date/abstract) and a rich doc
(2 structured authors w/ orcid+email+affiliations, date-modified, doi,
keywords, categories, license/copyright/citation, `title-block-banner: true`).
Q1 = quarto 99.9.9, Q2 = current `main`-ish branch.

Simple doc, element-by-element:

| Aspect | Q1 | Q2 today |
|---|---|---|
| header | `header#title-block-header.quarto-title-block.default` | same ✓ |
| subtitle | `p.subtitle.lead` | `p.subtitle` (no `lead`) |
| meta grid children | bare `<div>` per entry | `div.quarto-title-meta-author`, `div.quarto-title-meta-date` (extra classes; Q1 uses `.quarto-title-meta-author` for the *affiliations grid variant* instead — clash) |
| author contents | `<p>Norah Jones </p>` | bare text, no `<p>` |
| date | `<p class="date">July 1, 2026</p>` (date-format long forced) | bare text `2026-07-01`, unformatted |
| heading label | `Author`/`Authors` pluralized, localizable | hardcoded `Author` |
| abstract | `div > div.abstract > div.block-title` + `<p>` paragraphs | `div.abstract > div.abstract-title` + raw text (selector mismatch with ported SCSS, which styles `.block-title`) |

Rich doc: Q2 emits **none** of: banner (`div.quarto-title-banner`, header
relocation above `#quarto-content`, `main.quarto-banner-title-block`),
categories chips, authors/affiliations two-column grid, ORCID/email/URL
decorations, `date-modified`, `doi`, `keywords`. Structured authors render as
`truetrue` (bd-8v34zny5). Banner mode in Q1 additionally puts
`page-columns page-full` on the header and banner div.

Fixture location during study:
`<scratchpad>/titleblock/{simple,rich}.qmd` — to be turned into committed
test fixtures in Phase 0.

## Proposed architecture

Four moves, mirroring Q1's separation of concerns but respecting Q2's
constraints (single transform pipeline, **no DOM postprocessor**):

1. **Author/metadata normalization as a Rust transform** (Normalization
   phase, extending or sitting next to `MetadataNormalizeTransform`): a port
   of `authors.lua`'s normalization producing typed Rust structs
   (`Author`, `Affiliation`, …) that are then serialized back into meta as
   `authors`, `affiliations`, `by-author`, `by-affiliation`, and `labels` —
   the exact shapes Q1 templates consume. Format-agnostic: PDF/DOCX/JATS
   later get the same normalized model for free. The typed structs also feed
   `DocumentProfile.authors` (profile_version bump; new structured field
   rather than mutating the existing `Vec<String>`).
2. **Title block as doctemplate partials**, replacing the inline literal
   markup in `FULL_HTML_TEMPLATE`: `title-block.html`,
   `banner/title-block.html`, `title-metadata.html`, `_title-meta-author.html`
   ported nearly verbatim from Q1 (doctemplate is Pandoc-syntax compatible).
   This simultaneously gives users Q1-style `template-partials` override
   compatibility (the documented escape hatch for custom title blocks).
3. **Banner placement decided in the template, not a postprocessor**: Q1
   relocates the header with DOM surgery because Pandoc controls the
   skeleton; Q2's `FULL_HTML_TEMPLATE` *is* the skeleton, so a template
   conditional can emit the header before `#quarto-content` and add
   `quarto-banner-title-block` to `<main>` / `quarto-banner` to
   `#quarto-header` directly. Zero new architecture.
4. **Banner color/image via the existing SCSS variables + a small
   include-in-header style block** for explicit per-document colors/images,
   matching Q1's split: theme-derived defaults come from `bannerBg()`/
   `bannerColor()` (already in our ported SCSS); explicit
   `title-block-banner: "#FFDDFF"` / image paths produce a generated
   `<style>` include (Q2 already has include-in-header plumbing via
   `meta.rendered.includes.*`).

## Design decisions (settled with Carlos, 2026-07-15)

**Q1. DOM parity strictness — SETTLED.** Replicate Q1's element nesting and
class names exactly for everything inside `#title-block-header` (including
`subtitle lead`, `p.author`, `p.date`, `block-title`, bare-div meta
entries); the ported SCSS already targets those selectors. Q1's
whitespace/indentation quirks are not parity targets.

**Q2. Structured author model — SETTLED.** Typed structs in `quarto-core`
(new module, e.g. `metadata/authors.rs`), serialized into meta for templates
(Q1-shaped `by-author`/`by-affiliation`/`labels`), plus a new structured
field on `DocumentProfile` with a `profile_version` bump.

**Q3. Localization — SETTLED.** Hardcode English label defaults + support
the per-document `*-title` override options. Leave a written note in the
label module pointing at a future localization design (Q1's
`_language.yml` system) as its own epic.

**Q4. Date formatting — SETTLED.** Do **not** design a date-formatting
system in this epic. Extract/author one small shared date helper module and
make both the title block and listings/feeds consume it, so a future real
`date-format` design drops into a single seam. Current state found during
study: listings parse `date-format` into `ListingConfig`
(`crates/quarto-core/src/project/listing/config.rs:62,508`) but never
consume it — listing templates emit `$date$` raw
(`templates/item-default.template:50`), and the only date machinery in the
tree is `format_pub_date_rfc822` (feeds,
`listing/feed/binding.rs:483`, `time` crate). The shared helper starts as
minimal as we can get away with (parse ISO-ish inputs via `time`; render a
default human format for the title block); ambitions like Q1's token
system, locales, and `today`/`now`/`last-modified` keywords are explicitly
future work behind the same seam.

**Q5. Banner explicit-color mechanism — SETTLED (include-in-header
`<style>`, Q1-style), with the following clarification of how it coexists
with SCSS customization:**

- `title-block-banner: true` generates **no style block at all** — colors
  come purely from compiled SCSS via `bannerBg()`/`bannerColor()`
  (`$title-banner-bg` → `$navbar-bg` → `$body-color`), so the normal SCSS
  customization path (custom theme setting `$title-banner-*`) is untouched.
- An explicit color/image (`title-block-banner: "#FFDDFF"` / path) generates
  the block, which **deliberately** beats the theme (doc metadata > theme
  default), guaranteed twice over without `!important`: (a) source order —
  `$css$` theme links precede `$header-includes$` in `FULL_HTML_TEMPLATE`,
  so the generated block comes later in the cascade; (b) specificity — the
  generated rule targets `.quarto-title-block .quarto-title-banner`
  (0,2,0), beating the SCSS layer's `.quarto-title-banner` (0,1,0).
- Why not inject the color into the SCSS compile: theme CSS is compiled
  per-theme and shared/cacheable across a project's documents; per-document
  variables would force per-document compiles or fragment the cache.
- Known limitations (shared with Q1): SCSS functions can't react to the
  explicit color (`bannerDim()` stays theme-derived), and user CSS above
  (0,2,0) specificity can still override the generated block — the normal
  CSS escape hatch.

**Q6. `title-block-style: manuscript` — SETTLED.** Skip manuscript handling
entirely for now (no partials, no grid, no warning machinery beyond
whatever the schema layer does by default).

**Q7. `funding` — SETTLED.** Normalize the schema in the author-model pass
(shares `ref:` machinery); emit nothing in HTML.

**Q8. ORCID badge — SETTLED.** Inline SVG (not Q1's base64 PNG), keeping
the `quarto-title-author-orcid` class extensions target.

**Q9. q2-preview parity — SETTLED (lockstep).** `PreviewTitleBlock.tsx` is
updated in lockstep with every markup change in this epic, accepting the
toil, because it preserves the option of a future React upgrade where the
title block becomes **editable** in preview. Every phase that changes
title-block markup carries a preview work item.

**Q10. `citation` / Google Scholar meta tags — SETTLED.** Out of scope;
belongs to an appendix/citation epic.

**Q11. `description` in the title block — SETTLED.** Yes, in the
metadata-grid phase; the `hide-description` gate only if/when the website
pipeline sets it.

## Proposed phases

Integration branch: `feature/bd-gx9cic8z-title-block-parity`.
Authorization (Carlos, 2026-07-15): commit phase-by-phase and push to the
remote feature branch without per-commit check-ins; **do not open a PR**
until the epic is done and explicitly approved.

Phase strands
(all parent-child under bd-gx9cic8z, `blocks` deps encode the order
0 → 1 → {2 → 3 → 5, 4, 6} → 7):

| Phase | Strand |
|---|---|
| P0 harness | bd-xj96vafq |
| P1 DOM parity + partials | bd-tezzk9vp |
| P2 author model | bd-ez0hiowa |
| P3 metadata grid | bd-j6huijli |
| P4 date helper | bd-13f821l5 |
| P5 banner | bd-364ol5lu |
| P6 styles | bd-vkiwhcny |
| P7 docs | bd-y71ga2l8 |

TDD note: every phase starts with fixtures + failing snapshot/DOM
assertions, per project policy. End-to-end verification via
`cargo run --bin q2 -- render` + inspecting output HTML (and browser
screenshots for banner/visual phases) before any phase is declared done.

### Phase 0 — Test harness + fixtures (bd-xj96vafq) — DONE 2026-07-15

- [x] Commit title-block fixtures: smoke-all corpus at
      `crates/quarto/tests/smoke-all/title-block/` (simple-default,
      rich-authors, metadata-grid, banner-true, label-overrides,
      style-plain, style-none) with current-truth `ensureHtmlElements`
      assertions, strengthened per phase. (banner-color/banner-image
      fixtures arrive with P5, where their behavior first exists.)
      Harness validity proven: a deliberately-wrong selector fails.
- [x] Snapshot tests of the `#title-block-header` subtree:
      `crates/quarto-core/tests/integration/title_block_pipeline.rs`
      drives `ProjectPipeline`/`RenderToFileOptions` (the `q2 render`
      orchestrator path, not `HtmlRenderConfig::default()`); 8 insta
      baselines incl. the `<main>` open tag for banner. Verified the
      real binary's output matches the baseline byte-for-byte.
- [x] Q1-vs-Q2 comparison procedure + DOM diff tables:
      `claude-notes/research/2026-07-15-title-block-q1-q2-dom-diff.md`
- Note: existing `smoke-all/q2-preview/title-block-*.qmd` fixtures cover
  the preview React title block under the JS runners — the Q9 lockstep
  guard; extend those alongside each phase.

### Phase 1 — DOM parity for the existing surface (bd-tezzk9vp) — DONE 2026-07-15

- [x] `p.subtitle.lead`; author/date wrapped in `<p>`/`<p class="date">`;
      abstract as `div > div.abstract > div.block-title` with paragraph
      content; bare-div meta entries (dropped `quarto-title-meta-author/
      -date` classes — the former is reserved for the P2 affiliations
      grid); `quarto-title-meta` grid emitted whenever the title block
      renders (Q1 parity, empty grid allowed)
- [x] Author/Authors pluralization + `author-title`/`published-title`/
      `abstract-title`/`modified-title`/`doi-title`/`description-title`
      overrides, via new `AuthorsNormalizeTransform` (authors.lua-style:
      writes `by-author`, `labels`, `author-meta`, and
      `rendered.has-title-block` into meta; typed name parsing in new
      `quarto-core/src/metadata/authors.rs`). English defaults per Q3.
- [x] Converted the inline title block in `FULL_HTML_TEMPLATE` into
      built-in doctemplate partials (`title-block` / `title-metadata`,
      each also registered under the `.html` alias), user
      `template-partials` shadow them Q1-style; custom templates also
      resolve the built-ins as a final fallback
- [x] Lockstep: `PreviewTitleBlock.tsx` rewritten to the new markup,
      consuming the same derived meta (`by-author`/`labels`/
      `rendered.has-title-block`); its vitest integration suite and the
      five `smoke-all/q2-preview/title-block-*` fixtures updated;
      verified via Playwright smoke-all sweep (14 title-block tests)
- Bonus fixes landed here: bd-8v34zny5 (`truetrue`) fixed by the
  normalized author names (full model still P2); Q1-parity fixes for
  date-without-author (Published cell now renders) and
  no-title-with-authors (header renders without `<h1>`); the head
  `<meta name="author">` now emits one tag per author via Pandoc's
  `author-meta` convention.
- Known P1 deviations (intentional): date still unformatted (P4); one
  `<p>` per multi-paragraph abstract in *preview* only (noted in
  PreviewTitleBlock docs, follows P2 meta-fidelity work).

### Phase 2 — Structured author/affiliation model (bd-ez0hiowa) — DONE 2026-07-15

- [x] Ported authors.lua normalization to typed structs in
      `quarto-core/src/metadata/authors.rs`: full author schema
      (structured name with particles, degrees, orcid, email, phone,
      fax, url, acknowledgements, note with global numbering,
      attribute flags incl. `attributes:` list/map form, CRediT roles
      with alias + vocab decoration, metadata bucket), affiliation
      schema (all Q1 fields, `state`→`region` and
      `affiliation-url`→`url` aliases, metadata bucket), inline +
      `ref:` + top-level `affiliations:` block with dedup/remap,
      `funding` normalization (schema only, Q7). Documented
      deviations (in the module doc): BibTeX-heuristic name split
      instead of Q1's bibtex round-trip; literals include particles;
      proper base-26 letters; undefined `ref:` → warning + drop
      instead of Q1's abort; roles-map takes all entries.
- [x] `AuthorsNormalizeTransform` emits `authors` (refs),
      `affiliations`, `by-author` (denormalized), `by-affiliation`,
      `funding`, extended `labels` (+ `affiliations`
      single/plural + `affiliation-title` override), `author-meta`,
      `rendered.has-title-block`; surfaces normalization issues as
      render diagnostics.
- [x] Two-column authors/affiliations grid in the built-in
      `title-metadata` partial (Q1's `$if(by-affiliation)$` /
      `$elseif(by-author)$` split; `/first` pipe not needed since the
      key is only written when non-empty).
- [x] New `_title-meta-author` built-in partial: url link with
      degrees inside the anchor, email icon, ORCID badge — both
      inline SVGs (Q8; envelope from bootstrap-icons upstream, ORCID
      glyph in brand green #A6CE39). SCSS deviation documented in
      `title-block.scss`: `svg` joined `img` in the orcid rule +
      email svg sizing. **Doctemplate fix that fell out**: the
      bare-partial external scanner now accepts underscore-leading
      names (`$_title-meta-author()$` — Q1 template-partials
      compatibility), `crates/tree-sitter-doctemplate/grammar/src/scanner.c`.
- [x] `DocumentProfile` v7: `authors_structured: Vec<ProfileAuthor>`
      (+ `ProfileAffiliation`), flat `authors` now derives from the
      same model; contract doc change log updated (incl. retroactive
      v6 entry).
- [x] Lockstep: `PreviewTitleBlock.tsx` renders the two-column grid +
      decorations from the same derived meta; also fixed the P1-noted
      multi-paragraph-abstract fidelity gap (one `<p>` per paragraph).
      New q2-preview smoke fixture `title-block-rich-authors.qmd`;
      vitest suite grown to 26 tests.
- Snapshot/baseline changes: `title_block_rich_authors` insta
  snapshot re-captured (the intended new DOM); phase5 byte-identity
  `styles.css` hash re-captured (SCSS svg rules, entry documented in
  `expected_hashes.txt`).

### Phase 3 — Metadata grid completeness (bd-j6huijli) — DONE 2026-07-16

- [x] TDD red: strengthened `smoke-all/title-block/metadata-grid.qmd`
      assertions (p.date-modified, p.doi > a[href doi.org],
      div.keywords > div.block-title, div.description,
      div.quarto-categories > div.quarto-category; 6 checks red before
      the fix); new `categories-disabled.qmd` fixture
      (`title-block-categories: false` must NOT emit
      .quarto-categories); new insta case
      `title_block_metadata_grid_no_categories` in
      `title_block_pipeline.rs`
- [x] `date-modified` (Modified), `doi` (linked to doi.org) grid cells +
      `keywords` block in `TITLE_METADATA_PARTIAL`
- [x] `description` block + `hide-description` gate (ported verbatim;
      nothing sets the flag in Q2 yet, per Q11) and category chips
      (`div.quarto-categories > div.quarto-category`) in
      `TITLE_BLOCK_PARTIAL`; `AuthorsNormalizeTransform` writes
      `quarto-template-params.title-block-categories` (bool true,
      Q1's exact key so Q1-ported custom partials keep working;
      omitted when the document sets `title-block-categories: false`)
- [x] `description` joined `RICH_TITLE_BLOCK_FIELDS` (inline HTML in the
      title block); head `<meta name="description">` switched to the
      plain-text `description-meta` derived by
      `MetadataNormalizeTransform` (the Pandoc/Q1 head contract,
      explicit value wins); head keywords meta joins list values with
      `, ` ($for/$sep$)
- [x] `has_title_block_content` extended with description / doi /
      date-modified / keywords / categories
- [x] Lockstep: `PreviewTitleBlock.tsx` metadata grid additions (Q9):
      categories chips, description (+hide-description),
      Modified/Doi cells, keywords block; vitest integration suite
      grown by 7 tests (547 pass); new q2-preview smoke fixture
      `title-block-metadata-grid.qmd`; Playwright title-block sweep
      green (17 tests, after `npm run build:wasm` + `VITE_E2E=1
      npm run build`)
- [x] End-to-end (2026-07-16): rendered the metadata-grid fixture
      (description changed to `A *fine* one-line description.` to
      prove rich rendering) via
      `cargo run --bin q2 -- render <scratch>/doc.qmd` and inspected
      `doc.html`: title block contains
      `<div class="quarto-category">analysis</div>`,
      `<div class="description">\nA <em>fine</em> one-line
      description.`, `<p class="date-modified">2026-07-10</p>`,
      `<p class="doi"><a href="https://doi.org/10.1234/example.5678">`,
      and `<div class="keywords">…<p>music, texas</p>`; head has
      `<meta name="description" content="A fine one-line description.">`
      (plain text) and `<meta name="keywords" content="music, texas">`
- Snapshot changes: `title_block_metadata_grid.snap` updated (the
  intended new grid markup), `title_block_metadata_grid_no_categories.snap`
  added. Phase5 `expected_hashes.txt` NOT re-captured: its fixture doc
  has no description/keywords/categories metadata, so its bytes are
  unchanged (byte-identity test passed in the workspace run).

### Phase 4 — Date parsing & formatting (bd-13f821l5) — DONE 2026-07-17

Executed per the approved ancillary design
(`2026-07-17-date-formatting-design.md`); scope grew beyond the
original Q4 "helper only" sketch with Carlos's approval (all tokens
without i18n/localization design cost). Delivered:

- [x] `crates/quarto-core/src/dates.rs`: Q1's parse-form list (no
      guessing tail — diagnostic instead), named styles
      full/long/medium/short/iso (English, Q3 deferral), day.js token
      formatter with `[...]` escapes incl. ISO-week tokens; deferred
      locale-week (`w ww wo gggg`) + named-tz (`z zzz`) tokens warn.
      Unit matrix mirrors the Q1 docs-page examples.
- [x] `DateNormalizeTransform`: keywords (`today`/`now` via UTC
      runtime clock — `SystemRuntime::unix_timestamp`, WASM-safe —
      and `last-modified` via VFS-aware mtime), ISO
      `date-meta`/`date-modified-meta` (head dcterms switched to it —
      deliberate deviation, the machine slot stays ISO), in-place
      formatted `date`/`date-modified`, Q1's precedence (field
      `{value,format}` > `date-format` > forced `long` for the styled
      HTML title block, `iso` otherwise and for all other formats).
- [x] Listings format date fields at record-build (finally consuming
      `ListingConfig::date_format`; doc `date-format` fallback;
      `medium` default) — the same pre-template position as Q1's EJS
      records; feeds parse via the shared module.
- [x] TDD: new smoke fixtures `date-format.qmd` +
      `date-default-long.qmd` (red first: 5 regex checks); insta
      re-baselines (4 snapshots: `2026-07-01` → `July 1, 2026` etc.,
      matching the P0 research doc's Q1 extracts); listing test pins
      updated to `Jan 15, 2026`; 8 transform unit tests + 9 dates
      module tests. Playwright title-block sweep green (23 tests —
      the date fixtures run under the WASM runner).
- [x] E2E (2026-07-17): `date: today` + `date-format: "dddd MMM Do,
      YYYY"` renders `<p class="date">Friday Jul 17th, 2026</p>` with
      `<meta name="dcterms.date" content="2026-07-17T00:00:00+00:00">`;
      output inspected.
- Preview lockstep (Q9): no TSX changes by design — the preview
  pipeline runs the transform, so formatted dates flow through the
  existing meta reads.

#### (superseded original sketch)

> **Design study (2026-07-17, at Carlos's request):**
> `claude-notes/plans/2026-07-17-date-formatting-design.md` — a full
> study of Q1's date subsystem (source + quarto-web docs) and a
> proposed "familiar, not byte-for-byte" Q2 design. Headline finding:
> Q1 formats all dates *before* templating (EJS never formats), so
> doctemplates being logic-less is no obstacle — the Q2-natural
> transform-based design is structurally what Q1 already does. The
> items below predate that study; the ancillary doc's work-item list
> supersedes them once the design is approved.

- [ ] Create one shared date module (parse ISO-ish inputs via the `time`
      crate; render a default human display format) and consume it from the
      title block
- [ ] Point listings/feeds at the same module (listing `$date$` display and
      the existing `format_pub_date_rfc822` seam) so the future
      `date-format` design lands in one place; the parsed-but-unused
      `ListingConfig::date_format` stays unused until that design
- [ ] Module doc note pointing at Q1's `date-format` token/locale/keyword
      surface as the future design target

### Phase 5 — Banner mode (bd-364ol5lu) — DONE 2026-07-16

Design notes (scoped 2026-07-16):

- **One partial, internal branch.** The `title-block` partial branches
  on a derived `rendered.title-block-banner` flag rather than
  registering a separate `banner/title-block` partial: in Q1, a user's
  `template-partials` file named `title-block.html` shadows the
  built-in in *both* modes (Pandoc resolves partials by basename, and
  Q1's banner file is `banner/title-block.html`); a single Q2 partial
  name preserves exactly that override semantics. Q1's inert
  `quarto-template-params.banner-header-class` hook is ported verbatim
  (no producer yet — see toc-left deferral below).
- **Placement via skeleton conditionals** (architecture item 3):
  `FULL_HTML_TEMPLATE` emits `$title-block()$` before
  `<div id="quarto-content">` when the flag is set, inside `<main>`
  otherwise; `<main>` conditionally gains `quarto-banner-title-block`.
- **`page-columns page-full` baked into the partial markup** (header +
  banner div). In Q1 those classes come from the *generic* bootstrap
  grid DOM postprocessor (`ensureInGrid` walking up from
  `column-body`); with no DOM postprocessor we emit them directly —
  the P0 research doc's captured Q1 banner DOM is the target.
- **Banner styles**: `title-block-banner: true` → no generated style
  (theme SCSS `bannerBg()`/`bannerColor()`, already ported). Explicit
  color/image → a `<style>` block pushed onto
  `RenderContext.includes` (in-header), folded into
  `rendered.includes.*` at apply-template — the Q5 mechanism.
  `title-block-banner-color: body|body-bg` → no inline color (Q1's
  `titleColor()` returns undefined for both; the SCSS default chain
  handles them); any other value → `color:` on banner headings +
  container.
- **Image banner**: Q1's detection (absolute path, or exists relative
  to the input's dir) → `background-image: url(...)` +
  `background-size: cover` in the generated style; the file is copied
  to the output tree via a `ResourceCopyIntent` pushed on
  `RenderContext.resource_copies` (the `ResourceCollectorTransform`
  pattern).
- **Deferred, documented**: `#quarto-header.quarto-banner` — Q2's
  navbar has no `#quarto-header` wrapper element, and the class's only
  consumer in all of Q1 is `.quarto-banner nav.quarto-secondary-nav`
  (website secondary nav), which Q2 doesn't have; revisit when
  secondary-nav lands. `toc-left` (`banner-header-class`) — Q2 has no
  `toc-location` option at all yet (grep: zero hits), so the class
  would be unreachable; the template hook is in place for when
  toc-location lands.

Work items:

- [x] TDD red: strengthened `smoke-all/title-block/banner-true.qmd`
      (body > header.page-columns.page-full, banner div >
      quarto-title.column-body > h1, main.quarto-banner-title-block,
      header NOT inside main — 5 checks red before the fix); new
      fixtures banner-color.qmd + banner-image.qmd (+ committed 1x1
      banner.png); insta cases banner_color (header + generated style
      block), banner_image_style, plus `header_precedes_quarto_content`
      positional asserts and a non-banner stays-in-main guard
- [x] `TitleBannerTransform` (new, Normalization phase, HTML-format
      only — deliberately NOT revealjs): derives
      `rendered.title-block-banner`, classifies bool/color/image,
      generates the Q5 `<style>` appended to the canonical
      `rendered.includes.header` list (shared
      `append_to_rendered_header` with the favicon transform — reaches
      both the native `$header-includes$` and the q2-preview head
      injector), pushes the image `ResourceCopyIntent`; 9 unit tests
      for the classification matrix. **WASM note:** the image-vs-color
      existence probe goes through the injected `SystemRuntime`
      (`ShortcodeResolveTransform` pattern) — a bare `Path::is_file()`
      can't see `/project/` VFS files, which the Playwright sweep
      caught (banner-image.qmd failed under the WASM runner until the
      runtime probe landed)
- [x] `TITLE_BLOCK_PARTIAL` banner branch (Q1-verbatim, incl.
      description/categories inside the banner and no hide-description
      gate in banner mode); skeleton emits the partial above
      `#quarto-content` when the flag is set, `<main>` conditionally
      gains `quarto-banner-title-block`. Non-banner output is
      byte-identical (phase5 hash test passed unchanged).
- [x] Banner SCSS confirmed live: compiled theme CSS carries
      `.quarto-title-banner{...color:#fdfefe;background:#517699}`
      (theme-derived via bannerBg()/bannerColor())
- [x] Browser screenshot verification against Q1 (chrome-devtools MCP,
      Q1 = system `quarto` dev binary, 2026-07-16): banner-true doc
      renders visually identically (same slate banner, chips,
      grid-below layout); only expected deltas are the unformatted
      date (P4) and a few px of section-heading margin. Explicit-color
      doc verified: #FFDDFF banner + #111111 title beat the theme.
- [x] Lockstep (Q9): `PreviewTitleBlock.tsx` banner branch (shared
      `TitleMetaGrids` fragment mirrors Q1's shared title-metadata
      partial); `PreviewDocument.tsx` hoists the title block above
      `#quarto-content` + `<main>` class, mirroring the skeleton
      conditionals; +6 vitest cases (553 pass); new q2-preview smoke
      fixture `title-block-banner.qmd`; Playwright title-block sweep
      green (20 tests)
- [x] End-to-end (2026-07-16): `cargo run --bin q2 -- render` on an
      output-dir project with `title-block-banner: banner.png` +
      `title-block-banner-color: "#FFFFFF"`: `_site/banner.png` copied
      (ResourceCopyIntent), head `<style>` has
      `background-image: url(banner.png); background-size: cover;`,
      header before `#quarto-content`,
      `<main class="content quarto-banner-title-block">`; output
      inspected

Deferred (documented in the transform module doc + above):
`#quarto-header.quarto-banner`, `toc-left` producer.

### Phase 6 — Styles + degradation (bd-vkiwhcny) — DONE 2026-07-17

Design notes (scoped 2026-07-17):

- **Q1 semantics, verified from source**: `title-block-style: plain`
  keeps the full styled DOM but drops the `title-block.scss` layer
  from the CSS compile (`documentTitleScssLayer` returns undefined);
  `none` (or `false`) uses **Pandoc's fallback title block**
  (`formats/html/pandoc/title-block.html`: bare header without
  quarto classes, `h1.title`, `p.subtitle` without `lead`, one
  `p.author` per author, `p.date`, `div.abstract >
  div.abstract-title`) and also drops the SCSS layer; banner is
  disabled for `none` (`documentTitlePartial` returns no partials)
  but active for `plain`.
- **"Schema entries" resolution**: Q2 has no YAML schema-validation
  layer for document options — the established convention is a typed
  enum in `transforms/config.rs` (`AppendixStyle` precedent, which is
  exactly the Default/Plain/None shape needed, with silent fallback
  for unknown values — matching Q6's "no dedicated warning machinery"
  for `manuscript`).
- **SCSS layer toggle**: `ThemeConfig` (quarto-sass) gains
  `title_block_layer: bool` read from `title-block-style` in
  `from_config_value`; `assemble_theme_scss` honors it. The
  no-theme fast path (shared cached default CSS) additionally
  requires the flag; a `plain`/`none` doc takes the existing
  fingerprinted `compile_with_doc_vars` path (flag joins the cache
  hash), so the shared default bundle stays byte-identical and
  cacheable — at most one extra CSS variant per project.
- **Minimal-mode `TitleBlockTransform`**: orthogonal — it only fires
  for `minimal: true` / `theme: none|pandoc`; `title-block-style`
  operates in the full template. Documented, no code interaction.
- **Preview lockstep is markup-only** (per the Q9 item): `none`
  renders the Pandoc-fallback markup in `PreviewTitleBlock`; `plain`
  changes no markup (the CSS-layer drop is a render-CSS concern).

Work items:

- [x] TDD red: strengthened `style-none.qmd` (bare header, no
      `.quarto-title-block` class, `p.author`; 5 checks red) and
      `style-plain.qmd` (styled DOM + CSS negatives); insta
      `title_block_style_none` re-baselined to the Pandoc fallback.
      **Found while testing**: one responsive `.quarto-title-banner`
      margin rule lives in the *bootstrap* layer (in Q1 too —
      `_bootstrap-rules.scss:1916`), so it correctly survives
      plain/none; the CSS negatives target layer-only selectors
      (`.quarto-title-meta-heading`, `.quarto-title-author-orcid`)
- [x] `TitleBlockStyle` enum in `transforms/config.rs`
      (`AppendixStyle` pattern; `false` = none, `manuscript`/unknown →
      silent Default per Q6); `AuthorsNormalizeTransform` derives
      `rendered.title-block-none`; `TitleBannerTransform` skips when
      none (new pipeline test: none + banner → header stays in main,
      no banner markup)
- [x] `TITLE_BLOCK_PARTIAL` Pandoc-fallback branch
      (`$if(rendered.title-block-none)$`; iterates `by-author` names;
      `$labels.abstract$` where Pandoc uses `$abstract-title$` —
      documented deviation, override still honored)
- [x] `ThemeConfig.title_block_layer` (from `title-block-style`,
      duplicated reader documented) honored in `assemble_theme_scss` +
      both native and WASM `compile_with_doc_vars`; stage fast path
      requires the flag, cache key includes it; unit-test matrices in
      quarto-sass and quarto-core
- [x] Lockstep: `PreviewTitleBlock.tsx` none-branch (Q9); +2 vitest
      (555 pass); new q2-preview fixture `title-block-style-none.qmd`;
      Playwright title-block sweep green (21 tests, incl. the html
      style fixtures under the WASM runner — proving the WASM
      dart-sass path honors the flag)
- [x] End-to-end (2026-07-17): `cargo run --bin q2 -- render` of
      none/plain/default docs: none emits
      `<header id="title-block-header"><h1 class="title">…<p
      class="author">` (Pandoc fallback, no quarto classes); plain
      and none CSS contain 0 `quarto-title-meta-heading` rules vs 1
      in the default control; outputs inspected

### Phase 7 — Docs + wrap-up

- [ ] docs/ page for title blocks (rendered with q2), documenting supported
      surface and deviations from Q1
- [ ] Follow-up strands for deferred items (manuscript style, language-file
      localization, real date-format design on the Phase-4 seam,
      citation/scholar meta tags, editable React title block)

## References

- Q1 code: `external-sources/quarto-cli/src/format/html/format-html-title.ts`,
  `src/resources/formats/html/templates/*`, `src/resources/filters/modules/authors.lua`,
  `src/resources/schema/document-layout.yml`, `src/resources/language/_language.yml`
- Q1 docs: `external-sources/quarto-web/docs/authoring/title-blocks.qmd`,
  `docs/authoring/front-matter.qmd`, `docs/journals/authors.qmd`
- Q2 code: `crates/quarto-core/src/template.rs` (FULL_HTML_TEMPLATE:145-262,
  title block :216-245, RICH_TITLE_BLOCK_FIELDS:648),
  `crates/quarto-core/src/transforms/title_block.rs`,
  `crates/quarto-core/src/transforms/metadata_normalize.rs`,
  `crates/quarto-core/src/document_profile.rs` (:742-766 deferred author model),
  `resources/scss/html/templates/title-block.scss`,
  `crates/quarto-core/src/stage/stages/apply_template.rs` (:379-406 partials)
- Prior plans: `claude-notes/plans/2026-05-10-q2-preview-plan-2d-body-container.md`
  (preview title block), `2026-05-04-q2-preview-plan-2b-builtin-components.md`
- Strands: bd-gx9cic8z (this epic), bd-8v34zny5 (truetrue bug),
  bd-5706gcrq (rich inline markup in title fields)

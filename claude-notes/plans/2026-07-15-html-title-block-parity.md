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

### Phase 3 — Metadata grid completeness

- [ ] `date-modified` (Modified), `doi` (link to doi.org), `keywords`,
      `description` (+ Q11 gate)
- [ ] Category chips (`div.quarto-categories > div.quarto-category`) +
      `title-block-categories` option
- [ ] Lockstep: `PreviewTitleBlock.tsx` metadata grid additions (Q9)

### Phase 4 — Shared date helper (not a date-format system, per Q4)

- [ ] Create one shared date module (parse ISO-ish inputs via the `time`
      crate; render a default human display format) and consume it from the
      title block
- [ ] Point listings/feeds at the same module (listing `$date$` display and
      the existing `format_pub_date_rfc822` seam) so the future
      `date-format` design lands in one place; the parsed-but-unused
      `ListingConfig::date_format` stays unused until that design
- [ ] Module doc note pointing at Q1's `date-format` token/locale/keyword
      surface as the future design target

### Phase 5 — Banner mode

- [ ] `banner/title-block.html` partial; header emitted above
      `#quarto-content` via template conditional; `page-columns page-full`
      classes; `main.quarto-banner-title-block`; `#quarto-header.quarto-banner`
- [ ] `title-block-banner: true` (theme-derived via existing SCSS)
- [ ] Explicit color + `title-block-banner-color` (`body`/`body-bg`/color)
      via generated include-in-header style (per Q5)
- [ ] Image banner (path detection, `background-image`, resource collection
      so the image is copied to the output dir)
- [ ] `toc-left` header class when `toc-location: left`
- [ ] Browser screenshot verification against Q1 output
- [ ] Lockstep: `PreviewTitleBlock.tsx` banner variant (Q9)

### Phase 6 — Styles + degradation

- [ ] `title-block-style: plain` (structure without the SCSS layer) and
      `none` (verbatim/minimal behavior); interaction with existing
      minimal-mode `TitleBlockTransform`
- [ ] Schema entries for the option surface; `manuscript` skipped entirely
      per Q6 (no dedicated warning machinery)
- [ ] Lockstep: `PreviewTitleBlock.tsx` honors style option where it
      changes markup (Q9)

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

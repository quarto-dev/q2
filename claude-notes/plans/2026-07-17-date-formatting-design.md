# Date Parsing & Formatting for Q2 (title-block P4, bd-13f821l5)

**Status: DESIGN DRAFT (2026-07-17) — for iteration with Carlos;
execution awaits explicit go-ahead.**

Ancillary to the title-block parity epic plan
(`2026-07-15-html-title-block-parity.md`, design decision Q4 and the
Phase 4 section). Q4 settled that this epic does *not* build Q1's full
date-format system, only a shared seam; Carlos's P4 brief (2026-07-17)
asks for a deeper study of Q1's feature and a **"familiar" design** —
recognizably the same option surface, not byte-for-byte — with
particular attention to the EJS-templates question.

## The headline finding: EJS is not an obstacle, because Q1 never
## formats dates at template time

The concern motivating this study was that Q1's listing templates are
EJS (executable JavaScript) while Q2 uses doctemplates (logic-less,
Pandoc-compatible), so Q1's technique might not carry over. Reading
the Q1 source shows the concern dissolves: **Q1 itself formats every
date before any template runs.**

- **Document metadata** (`src/command/render/pandoc.ts:1186-1197`):
  before invoking Pandoc, Q1 *replaces* `date` and `date-modified` in
  the metadata with the formatted string
  (`resolveAndFormatDate(source, date, format)`). The Pandoc template
  only ever interpolates `$date$` as an opaque, pre-formatted string.
  Default format when `date-format` is absent: **`iso`** — so every
  render normalizes `03/07/2005` → `2005-03-07` even with no options
  set.
- **Listings** (`website-listing-template.ts:82-112`): the EJS
  *record* is built in TypeScript first; date-typed fields
  (`field-types` `date`) are parsed and formatted into the record
  **before** EJS interpolates them. Default `medium`; the
  `file-modified` field gets `short` date + `medium` time. The EJS
  template body does no date logic at all.
- **Title block's "July 1, 2026"**: `documentTitleMetadata()`
  (`format-html-title.ts`) injects `date-format: long` into format
  metadata when the styled HTML title block is active and the user
  set no `date-format`. The pre-Pandoc rewrite then does the rest.

So the Q2-natural design — format in a **Normalization-phase
transform** that writes plain strings into metadata, keeping
doctemplates logic-less — is not a compromise; it is structurally the
same pipeline position Q1 uses. Everything downstream (built-in
partials, user `template-partials`, the q2-preview React title block,
listings' doctemplates) gets formatted dates for free.

## Q1 feature surface (source + docs study)

Sources: `src/core/date.ts`, `src/command/render/pandoc.ts`,
`src/format/html/format-html-title.ts`,
`src/project/types/website/listing/website-listing-template.ts`,
`src/command/render/render-files.ts:560-572`; docs:
`quarto-web/docs/reference/dates.qmd` (the authoritative user
contract), `docs/authoring/title-blocks.qmd`,
`docs/websites/website-listings.qmd`.

1. **Parsing** (`parsePandocDate`): tries, in order, `MM/dd/yyyy`,
   `MM-dd-yyyy`, `MM/dd/yy`, `MM-dd-yy`, `yyyy-MM-dd`, `dd MM yyyy`,
   `MM dd, yyyy`; then a *guessing* library (`moment-guess`); then
   JS `new Date()` (which accepts ISO timestamps, RFC formats, etc.).
2. **Keywords** (`today`, `now`, `last-modified`): resolved before
   parsing. `last-modified` = max mtime of the input file(s);
   `today` = local date at 00:00; `now` = current instant. All are
   materialized as ISO timestamps first, then formatted.
3. **Named styles**: `full` / `long` / `medium` / `short` via
   `Intl.DateTimeFormat` (locale-aware), `iso` = `YYYY-MM-DD`.
4. **Format strings**: day.js tokens (documented table in
   `dates.qmd`): `YY YYYY M MM MMM MMMM D DD d dd ddd dddd H HH h hh
   m mm s ss SSS Z ZZ A a` plus plugin tokens `Q Do k kk X x w ww W
   WW wo gggg GGGG z zzz`; `[...]` escapes literal text.
5. **Per-field override**: `date` may be a map
   `{ value: ..., format: ... }` — the field-local format beats the
   document `date-format`.
6. **Locale**: `setDateLocale(lang)` (from the `lang` option) loads a
   day.js locale file; named styles and month/day names localize.
7. **Fields covered**: `date` and `date-modified` in document
   metadata; every `field-types: date` column in listings
   (`date`, `date-modified`, `file-modified`, custom fields).
8. **Adjacent but separate** (not part of this design):
   citation dates → CSL date objects (`cslDate`, same pandoc.ts
   block); RSS feeds use their own RFC-2822 formatting (Q2 already
   ports this as `format_pub_date_rfc822`).

One Q1 behavior worth calling out: because `date` is replaced
in-place *before* Pandoc, Pandoc's derived `date-meta` — and thus the
`<meta name="dcterms.date">` head tag — carries the **human-formatted**
string (e.g. `March 7, 2005`) whenever `date-format` is set. That is
arguably a wart: the machine-readable slot loses its ISO form.

## Q2 current state

- Title block emits `$date$` / `$date-modified$` raw and unformatted
  (`2026-07-01`) — the one remaining visible delta from Q1 in the
  P0/P5 comparisons.
- `ListingConfig::date_format` is parsed (`listing/config.rs:62,508`)
  but never consumed; listing templates interpolate `$date$` raw
  (`templates/item-default.template:50`). `field-types` already has
  `ColumnType::Date` (`config.rs:670`).
- Feeds format pub dates via `format_pub_date_rfc822`
  (`listing/feed/binding.rs:483`, `time` crate) with an ad-hoc parse.
- `listing_item_info.rs` already derives `date-modified` from the
  **runtime** mtime (`mtime_iso(ctx.runtime...)`) — i.e., a VFS-aware
  mtime primitive exists and works in WASM; `last-modified` can reuse
  it.
- `time = "0.3"` (formatting + parsing + macros) is already a
  quarto-core dependency. No chrono/icu/jiff in the tree.
- P3 established the `description`/`description-meta` split precedent
  (rich value for the body, plain derived value for the head) and
  MetadataNormalizeTransform as the home for derived plain-text meta.
- The q2-preview title block reads `meta.date` directly — since the
  preview pipeline runs the same Normalization transforms, a
  transform-based design gives the preview formatted dates with **no
  TSX changes** (tests only).

## Proposed design

### 1. One date module: `crates/quarto-core/src/dates.rs`

Pure functions plus an injectable "now"/mtime boundary:

- `parse_date(&str) -> Option<ParsedDate>` — Q1's explicit format
  list plus ISO-8601 date/timestamp forms, implemented with `time`'s
  `format_description!` parsers. **Deviation**: no guessing library —
  Q1's `moment-guess` tail is replaced by "explicit formats + ISO,
  else a render diagnostic naming the accepted forms". A warning that
  names the supported formats is friendlier than Q1's silent
  guesswork, and drops a whole dependency class.
  `ParsedDate` wraps a `time::OffsetDateTime` (or
  `PrimitiveDateTime` + optional offset) and remembers whether a time
  component was present (listings' file-modified wants date+time,
  plain dates don't).
- `resolve_keyword(&str, ctx) -> Option<String>` — `today` / `now` /
  `last-modified`, materialized as ISO timestamps like Q1.
  `last-modified` uses the runtime mtime primitive (the
  `listing_item_info::mtime_iso` seam, generalized); `today`/`now`
  come from an injectable clock (a `fn() -> OffsetDateTime` or small
  trait) so tests are deterministic and WASM works.
- `format_date(&ParsedDate, style: &DateStyle) -> String` where
  `DateStyle` parses from the option string:
  - Named styles `full | long | medium | short | iso`, **English
    only** for now: `full` = `Monday, March 7, 2005`, `long` =
    `March 7, 2005`, `medium` = `Mar 7, 2005`, `short` = `3/7/05`,
    `iso` = `2005-03-07` — matching Q1's `en` Intl output shapes.
    Rationale: epic decision Q3 already hardcodes English title-block
    labels with localization deferred to its own epic; date locales
    (Q1: day.js locale files + `Intl`) join that epic. This is the
    module's designed extension point — `DateStyle` + a future locale
    parameter, one seam.
  - **Token strings**: a small tokenizer for the day.js grammar with
    `[...]` escapes. Proposed supported subset (covers every example
    in Q1's docs page and everything the built-in templates/listings
    ever emit): `YYYY YY MMMM MMM MM M DD D dddd ddd dd d HH H hh h
    mm m ss s SSS A a Do Z ZZ Q k kk X x`, **plus the ISO-week
    tokens `W WW GGGG`** — those are nearly free (`time` has
    `iso_week()` and ISO week-year built in, no locale data
    involved). Deferred (warn + render literally), by cost class:
    - *locale-week tokens* `w ww wo gggg` — "which day starts the
      week / which week is week 1" is locale-defined (US vs ISO
      rules differ), so these belong to the deferred localization
      epic alongside named-style locales;
    - *named-timezone tokens* `z zzz` ("EST" / "Eastern Standard
      Time") — need a full IANA tz database, a heavyweight
      dependency `time` deliberately omits, for a token with no
      plausible document-date use.
    Unknown runs of alpha characters outside `[...]` produce one
    diagnostic naming the token.

Placement note: quarto-core (not quarto-util) because the keyword
resolution needs `SystemRuntime` and render diagnostics; the pure
parse/format core could migrate down later if a second crate needs it.

### 2. A `DateNormalizeTransform` (Normalization phase)

Runs next to `AuthorsNormalizeTransform` (order before it doesn't
matter; both only touch meta). For each of `date`, `date-modified`:

1. Accept scalar or Q1's `{ value, format }` map form.
2. Resolve keywords, parse; on parse failure: render diagnostic,
   leave the raw string untouched (Q1 silently emits `Invalid Date`
   in some paths — we can do better).
3. Write **`date-meta`** (and `date-modified-meta`): the ISO form
   (`YYYY-MM-DD`, or full timestamp when a time was present). The
   head's `<meta name="dcterms.date">` switches from `$date$` to
   `$date-meta$`. **Deliberate deviation from Q1**: machine slots
   keep ISO even when `date-format: long` is set (Q1 leaks the
   pretty string into dcterms). Mirrors the P3
   `description`/`description-meta` precedent exactly.
4. Replace `date` / `date-modified` **in place** with the formatted
   string (Q1-familiar: every downstream consumer — built-in
   partials, user template-partials, q2-preview — sees the formatted
   value with zero further changes).
5. Format selection, matching Q1's precedence: field-local
   `format` > document `date-format` > default. Default is
   **`long` when the styled HTML title block is active**
   (format-html + `title-block-style` ∉ {plain? see open question
   Q-c, none}) and **`iso` otherwise** — Q1's
   `documentTitleMetadata` rule plus its global iso normalization.

### 3. Listings and feeds consume the same module

- `listing_item_info` / the listing record builder formats every
  `ColumnType::Date` field: listing-level `date-format` (finally
  consuming `ListingConfig::date_format`) > document `date-format` >
  `medium` default; `file-modified` renders date+time (Q1's
  `short`+`medium` composite). Formatting happens at record-build
  time — the doctemplate listing templates keep interpolating plain
  strings, exactly like Q1's EJS.
- Feeds keep RFC-2822 output (a machine format, not user-styled) but
  `format_pub_date_rfc822`'s ad-hoc parse is replaced by
  `dates::parse_date`, closing the "two parsers drift" hole.

### 4. Preview (Q9) and testing

- No TSX changes: the preview pipeline runs the transform, so
  `PreviewTitleBlock` receives formatted `date`/`date-modified`
  strings as-is. Lockstep work is test-only (vitest fixtures gain
  formatted expectations; one new q2-preview smoke fixture with
  `date-format`).
- Determinism: all committed fixtures use literal dates. Keyword
  tests inject the clock/mtime; no snapshot ever contains `now`.
- The P0 insta baselines for `simple`, `metadata-grid`,
  `banner-true`, etc. will change (`2026-07-01` → `July 1, 2026`) —
  that diff *is* the review artifact and closes the last visible Q1
  delta from the P0/P5 comparisons.

## Summary of deviations from Q1 (the "familiar, not byte-for-byte" list)

| # | Q1 | Proposed Q2 | Why |
|---|---|---|---|
| 1 | `moment-guess` fallback parsing | Explicit formats + ISO, else diagnostic | Predictability; drops a guessing heuristic that can mis-read ambiguous dates |
| 2 | Named styles localize via `lang` + day.js locale files | English-only now; locale joins the localization epic (Q3 precedent) | One localization seam for labels *and* dates |
| 3 | Full day.js token set incl. week-of-year, named tz | Documented subset; deferred tokens warn | `time` has no tz db / week-year machinery; no real-world usage in Q1's own templates |
| 4 | `dcterms.date` gets the human-formatted string | `date-meta` stays ISO | Machine slots stay machine-readable (P3 precedent) |
| 5 | Silent `Invalid Date` on unparseable input (some paths) | Render diagnostic, raw string preserved | Q2 diagnostics culture |

## Open questions for Carlos

- **Q-a (in-place replace)**: OK to replace `date`/`date-modified`
  in meta with formatted strings (Q1-familiar), with `*-meta` ISO
  derivatives for machine slots? The alternative — derived
  `rendered.date-formatted` keys with raw meta untouched — is
  "cleaner" but breaks the familiar contract that `$date$` in a user
  template-partial is formatted, and forces every consumer to know
  two keys.
- **Q-b (non-HTML formats)**: Q1 normalizes dates to `iso` for every
  format (PDF, DOCX…) even without `date-format`. Match that
  (recommended: it's the same transform, gated on nothing), or scope
  the transform to HTML for now?
- **Q-c (forced `long` and `title-block-style: plain`)**: Q1 forces
  `long` for any styled title block (`default`, `plain`, banner;
  not `none`/`false`). Match exactly (recommended), or only for
  `default`?
- **Q-d (token subset)**: resolved refinement (2026-07-17): ISO-week
  tokens (`W WW GGGG`) move into the day-one subset — `time` provides
  them without locale data. Locale-week (`w ww wo gggg`) and
  named-timezone (`z zzz`) tokens stay deferred with diagnostics
  unless a concrete use case surfaces (a user writing e.g.
  `date-format: "[Week] w, YYYY"` for editorial/journal-style
  "Week 23, 2026" dating is the only scenario these serve).
- **Q-e (module home)**: `quarto-core::dates` as proposed, or start
  it in `quarto-util` with the runtime-dependent keyword resolution
  staying in quarto-core?

## Proposed work items (once approved)

1. TDD red: extend `smoke-all/title-block/metadata-grid.qmd` (or a
   new `date-format.qmd` fixture) with formatted-date assertions
   (`July 1, 2026`, `date-format: "MMM D, YYYY"` variant, `iso`
   head meta); listing fixture with a date column + `date-format`.
2. `dates.rs`: parser + keyword resolver + `DateStyle` formatter,
   unit-test matrix mirroring the docs page's examples table
   (including bracket escaping and the ordinal `Do`).
3. `DateNormalizeTransform` + head template switch to `$date-meta$`;
   re-baseline the affected insta snapshots (documented diff).
4. Listings: consume `ListingConfig::date_format` at record-build;
   feeds parse via the shared module.
5. Preview lockstep tests + Playwright sweep; end-to-end renders
   inspected (incl. a `date: today` doc — visually, not snapshot).
6. Docs note for P7: Q2's dates page = Q1's `dates.qmd` minus the
   deferred tokens/locale rows, plus the deviation table above.

## References

- Q1: `src/core/date.ts`, `src/command/render/pandoc.ts:1185-1226`,
  `src/format/html/format-html-title.ts` (`documentTitleMetadata`),
  `src/project/types/website/listing/website-listing-template.ts:75-118`,
  `src/command/render/render-files.ts:560-572` (`setDateLocale`).
- Q1 docs: `external-sources/quarto-web/docs/reference/dates.qmd`
  (parse forms, keywords, style + token tables),
  `docs/authoring/title-blocks.qmd`,
  `docs/websites/website-listings.qmd`.
- Q2: epic plan `2026-07-15-html-title-block-parity.md` (Q4, Phase
  4), `crates/quarto-core/src/project/listing/config.rs:62,508`,
  `crates/quarto-core/src/stage/stages/listing_item_info.rs`
  (`mtime_iso`), `crates/quarto-core/src/project/listing/feed/`
  (`format_pub_date_rfc822`), P3's `description-meta` precedent in
  `crates/quarto-core/src/transforms/metadata_normalize.rs`.

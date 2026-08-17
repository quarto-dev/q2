# Vendor and integrate quarto-listing.scss (bd-57y4)

**Strand:** bd-57y4 (P2; discovered from L3 phase 7, bd-ml8z — see D5 in
`claude-notes/plans/2026-05-06-listings-L3-resolve-transform.md`)
**Created:** 2026-07-29
**Branch:** `braid/bd-57y4-listing-scss` (based on the blog-scaffold
branch, PR #434 — independent code, but the blog scaffold is the natural
e2e fixture)

## Overview

Q2 ships the listing JS pair (`list.min.js` + `quarto-listing.js`) as
Project-scoped artifacts but never compiled Q1's `quarto-listing.scss`,
so listing cards/tables/category chips render with default browser
styling. This strand vendors the SCSS and composes it into the theme-CSS
bundle as a `SassLayer`, restoring Q1 visual parity for listings and the
categories sidebar (the L5 markup already exists and consumes these
classes).

## Design decisions

1. **Unconditional layer (deviation from the strand sketch, matching
   Q1).** The strand suggested adding the layer "when at least one
   listing is rendered", but:
   - Q1 attaches `listingSassBundle()` gated only on
     `formatHasBootstrap(format)` — *not* on `pageHasListings`
     (`website-listing.ts:284,424`); only the JS/postprocessor slots are
     page-conditional. Q2 parity = unconditional.
   - `CompileThemeCssStage` runs at stage 11, before the listing
     transforms populate `ctx.resolved_listings` (stage 13) — the only
     sound predicate would be raw `meta.listing`, which needs a new
     `ThemeConfig` flag plus a cache-key discriminator
     (`compile_theme_css.rs:234-238`), and would fork a website's theme
     CSS into two fingerprints (listing vs non-listing pages), worse
     for caching than one shared file.
   - Cost of unconditional: a few KB of CSS on non-listing docs — the
     same cost Q1 accepts.
2. **Vendor verbatim** to `resources/scss/html/templates/
   quarto-listing.scss` (auto-embedded via `TEMPLATES_DIR` include_dir,
   auto-hashed by quarto-sass `build.rs`). The file's
   `/*-- scss:variables --*/` marker is *invalid* (only
   uses/functions/defaults/mixins/rules parse), so its `!default` block
   lands in the functions band — the same known quirk as
   `title-block.scss` (`bundle.rs:178-189`, quarto-cli#13960). Q1's
   parser has the identical behavior; do NOT "fix" the marker.
3. **HTML paths only** — the layer is pushed at every HTML assembly
   site (`assemble_theme_scss`, native+wasm `compile_with_doc_vars`,
   native+wasm `compile_default_css`); **not** `assemble_reveal_scss`
   (no listings in slide decks).
4. **`$theme-name` parity (small extra).** Q1 sets `$theme-name` so the
   listing SCSS's per-theme override map (cyborg/darkly/slate/… chip
   borders and form colors) activates. Q2 never defines it; the layer's
   own `$theme-name: null !default` then makes every override fall back
   to defaults — correct for custom themes, a visible gap for the dark
   built-ins. Since Sass `!default` also fires on *null* values, a
   later-band `$theme-name: "<name>" !default` emitted for built-in
   themes activates the map. Implement if it stays a ~few-line change
   in the theme defaults; otherwise file a follow-up strand and ship
   without it.

## Work Items

### Phase 1: tests first (TDD)

- [x] T1 `quarto-sass` unit tests (mirror the highlight/copy-code
      regression pattern at `compile.rs:857-876`): compiled theme CSS
      contains `.quarto-listing` and listing-category rules for (a) the
      default-css path and (b) the themed/doc-vars path. One assertion
      per assembly path touched.
- [x] T2 e2e integration test (brand_render.rs shape, using
      `listing_pipeline.rs`'s `render_project` harness): render a
      website with a listing page → concatenated `.css` under the
      output tree contains `.quarto-listing` / `listing-category`
      selectors.
- [x] T3 ($theme-name, if implemented) unit test: compiling theme
      `darkly` yields the darkly category-chip override (border color
      `$gray-600`) rather than the default.
- [x] T4 run tests, record expected failures.

### Phase 2: implementation

- [x] I1 copy `external-sources/quarto-cli/src/resources/projects/
      website/listing/quarto-listing.scss` →
      `resources/scss/html/templates/quarto-listing.scss` (verbatim);
      update `resources/scss/README.md`'s vendored list.
- [x] I2 `load_listing_layer()` in `quarto-sass/src/bundle.rs` next to
      `load_copy_code_layer`; push at the HTML assembly sites
      (`compile.rs:88-98`, `:226-243`, `:359-370`, wasm `:498-515` and
      wasm `compile_default_css`).
- [x] I3 ($theme-name) emit `$theme-name: "<name>" !default;` into the
      defaults band for built-in bootstrap themes.
- [x] I4 make Phase-1 tests green; full `-p quarto-sass` +
      `-p quarto-core` suites (preview/render CSS parity tests must
      stay green — the layer is unconditional so fingerprints move
      uniformly).

### Phase 3: verification + handoff

- [x] V1 `cargo build --workspace` + `cargo nextest run --workspace`.
- [x] V2 full `cargo xtask verify` (quarto-sass feeds the WASM leg; the
      hub preview compiles SCSS through the dart-sass JS bridge — the
      layer must ride the assembled string, which SassLayer composition
      guarantees).
- [x] V3 e2e: `q2 create project blog myblog && q2 render myblog`,
      inspect `_site/` theme CSS for `.quarto-listing` rules and load
      the listing page to confirm card layout (image-right float,
      category chips, pagination styling); check a dark theme (darkly)
      if T3/I3 landed.
- [x] V4 update this plan, close bd-57y4 (note the unconditional-layer
      deviation in the close reason), report.

## Implementation record (2026-07-29)

- T4 recorded failures: 3 quarto-sass unit tests + the e2e test all
  failed before wiring (missing `.quarto-listing` selectors).
- I3 came **for free**: Q2's vendored bootstrap layer already defines
  `$theme: "<name>" !default` per built-in theme and derives
  `$theme-name` in `_bootstrap-variables.scss`, so the listing
  override map fires with no new code. One subtlety, captured in the
  darkly test: the override's `$gray-600` resolves to the
  *bootstrap-default* value (#6c757d), not darkly's, because the
  file's invalid `scss:variables` marker parks its `!default` block in
  the functions band ahead of theme defaults — Q1's parser has the
  identical band order, so this is parity, not a bug.
- V3 e2e (real binary): `q2 render` of the bd-r1by4u2a blog scaffold →
  `site_libs/quarto/quarto-theme-<fp>.css` contains the card layout
  (`div.quarto-post .thumbnail{flex-basis:30%…}`,
  `.quarto-listing-default…`, category-chip rules) and is linked from
  the listing page; markup classes (`quarto-post image-right`,
  `listing-categories`) match. Output inspected.
- Tests: quarto-sass 210/210; e2e + both preview/render CSS-parity
  guards green (the layer is unconditional, so fingerprints moved
  uniformly in both pipelines).
- V1/V2 (2026-07-29): workspace 10794/10794; full `cargo xtask verify`
  "All verification steps passed!". One legitimate baseline update: the
  phase5 single-doc byte-identity fixture's `styles.css` hash
  re-captured per its documented procedure (listing rules now in every
  HTML theme compile; `doc.html` hash unchanged — no listing selector
  matches a single-doc body).

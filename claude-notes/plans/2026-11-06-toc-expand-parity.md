---
title: "toc-expand: finish Quarto 1 parity"
---

**Strand:** bd-kr6721bn. **Status:** approved 2026-11-06; implemented.
**Related:** bd-toc-scrollspy-js-41e624d8 (PR #989, merged 2026-10-30).

## Overview

The request was to port Quarto 1's `toc-expand` option. Most of it is
already ported. It landed with the TOC scroll-tracking work in PR #989. This
plan covers the edge cases that PR left open.

### What Q1 does

- **Schema** (`quarto-cli/src/resources/schema/document-toc.yml`):
  `toc-expand: number | boolean`, default `1`, for `$html-doc` formats.
- **Render step** (`format-html-bootstrap.ts`, around line 402): writes
  `data-toc-expanded` on `nav#TOC` only when the key is set. `true` becomes
  `99`. Any other truthy value is written as is. Any falsy value (`false`,
  and also `0`) becomes `-1`.
- **Script** (`resources/formats/html/quarto.js`, lines 771 to 850): reads the
  attribute, using `1` when it is missing. It then walks the TOC on load,
  scroll and resize. A nested `<ul>` stays open when its depth is at most the
  open depth, when it holds the active link, or when it directly follows the
  active link. With `-1`, every nested list is collapsed.
- **Docs** (`quarto-web/docs/output-formats/html-basics.qmd`): one paragraph
  and a `toc-expand: 2` example.

### What q2 already has (verified on `main` at b3a25f6b8)

| Piece | Where |
|---|---|
| Value → depth mapping (`true` → 99, `false` → -1, an integer is used as is, the string `"3"` is accepted) | `toc_expand_depth`, `crates/quarto-core/src/transforms/toc_render.rs:283` |
| `data-toc-expanded` on the top-level `<ul>`, shared by all three `nav#TOC` emitters | `render_toc_level`, same file |
| The collapse walk (reads the attribute from `nav#TOC` or a descendant) | `resources/js/quarto-toc/quarto-toc.js:155` |
| `props.expanded` for `toc-component` | `crates/quarto-core/src/transforms/toc_js.rs:171` |
| Preview (renders the same HTML and injects the same script) | `TocSlot`, `ts-packages/preview-renderer/src/q2-preview/chromeSlots.tsx` |
| Tests | unit tests in `toc_render.rs` and `toc_js.rs`; `crates/quarto-core/tests/integration/toc_js_pipeline.rs`; smoke-all `crates/quarto/tests/smoke-all/toc/toc-expand-{default,depth,false}.qmd` |
| User docs | `docs/guides/authoring/navigation.qmd` § Following the reader |

I checked the following by hand with `target/debug/q2 render` (the scratch
fixtures are not committed):

- `format: html: toc-expand: 2` → `data-toc-expanded="2"`.
- A website with `toc-expand: 2` and `toc-location: left` in `_quarto.yml`
  → the attribute is present, and `site_libs/quarto-toc/quarto-toc.js` is
  shipped.
- `toc-location: body` with `toc-expand: true` → `data-toc-expanded="99"`.

## Remaining gaps

1. **`toc-expand: 0` behaves differently.** Q1 treats `0` as falsy and writes
   `-1`, so it behaves like `false`. q2 writes `"0"`. The script then reads
   depth 0, which never matches the forced-collapse branch. The result is
   that nested lists still open around the active section, the same as the
   default.
2. **Floats are dropped without a warning.** `toc-expand: 2.5` is a valid
   `number` in Q1, and it behaves like `2` because the script compares
   `depth <= 2.5`. q2's `as_int_lenient` rejects `Yaml::Real`, so no attribute
   is written and the TOC falls back to the default of 1.
3. **Invalid values are dropped without a warning.** `toc-expand: sideways`
   fails Q1's schema validation. In q2 it is ignored with no diagnostic. The
   neighbouring option `toc-location` warns about a bad value (Q-13-8).
4. **Negative integers other than -1.** Q1 writes them as is. With
   `-2` the forced-collapse branch does not apply, so the TOC behaves like
   the default. q2 currently does the same. This is undocumented and an
   accident in both versions.

## Proposed changes

All Rust changes are in `crates/quarto-core/src/transforms/toc_render.rs`
unless noted.

1. **Normalize the values in one place.** `toc_expand_depth` should return a
   classified result, so the warning and the depth come from the same code:
   - `true` → 99, `false` → -1 (no change).
   - Integer `n >= 1` → `n`. The lenient string `"3"` stays accepted.
   - `0` → -1, following Q1's falsy rule.
   - Finite float `x >= 1` → `floor(x)`, which gives the same behavior as Q1.
     A float below 1 is handled as its integer part would be.
   - Negative values other than -1, and every other kind (strings that are
     not integers, maps, lists) → `None`, plus a warning.

   `toc_js.rs` keeps calling the same function, so `props.expanded` agrees
   with the attribute.
2. **Add a new warning, Q-13-19, "Invalid toc-expand value".** Add it to
   `crates/quarto-error-catalog/error_catalog.json` and add
   `docs/errors/navigation/Q-13-19.qmd`, modelled on Q-13-8. Emit it once, from
   `TocRenderTransform` only. `TocJsTransform` reads the same value and must
   not report it a second time. Point the diagnostic at the value's source
   location, the way `toc_location.rs` does. The render continues with the
   default depth. **Check that Q-13-19 is still free just before landing.**
   Other in-flight branches allocate Q-13 codes; the announcement work took
   16 and 17.
3. **Tests.**
   - Unit tests: extend `test_toc_expand_values_map_like_q1` with `0` → -1 and
     `2.5` → 2. Add a test that the invalid values (`"sideways"`, `-3`, a map)
     produce `None` and exactly one Q-13-19.
   - Smoke-all: add `crates/quarto/tests/smoke-all/toc/toc-expand-invalid.qmd`
     that asserts the warning and the missing attribute. Add `toc-expand: 0` to
     the existing `toc-expand-false.qmd`, or to a sibling fixture.
4. **Docs.** In `docs/guides/authoring/navigation.qmd`, add a short note to
   the `toc-expand` table: `0` behaves like `false`, and other invalid values
   produce a warning and use the default.

No changes are needed to the JS, the preview or the templates. The script
already handles every depth that the normalized mapping produces.

## Decisions (Carlos, 2026-11-06)

1. **Floats** are rounded down, to match Q1.
2. **The lenient string `"3"`** stays accepted, with no warning.
3. **Negative values below -1** warn (Q-13-19) and use the default.
4. **A dedicated code**, Q-13-19, rather than a generic diagnostic.

Two smaller choices made during implementation:

- A YAML `null` (`toc-expand:` with no value) counts as unset, with no
  warning. Q1 would have treated it as falsy (`-1`), but an empty key
  reads as "not set" more naturally.
- Non-finite strings that Rust parses as floats (`"inf"`, `"NaN"`) are
  invalid.

## Discovered work

- bd-n0ym70qi: sidebar `contents: auto` reports Q-13-6 on a website whose
  only document is the root `index.qmd`, which Q1 excludes from `auto:`
  without a warning. Found while checking `toc-expand` on a website.

## Out of scope

- The other parts of `quarto.js` that are still unported: the overlap rollup
  (`hideOverlappedSidebars`) and reader mode. Both are listed as out of scope
  in bd-toc-scrollspy-js-41e624d8.
- `sidebar.collapse-level`, the website sidebar's own expand option, which is
  a separate option.

## Verification

- `cargo nextest run -p quarto-core toc_` and the smoke-all `toc/` fixtures.
- `cargo xtask verify --skip-hub-build`.
- `cargo run -q --bin q2 -- render --strict claude-notes`, which checks this
  file.

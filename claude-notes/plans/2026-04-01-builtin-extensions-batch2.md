# Plan: Built-in Extensions Batch 2 — version, kbd, placeholder

## Status: Complete

## Prerequisites

- **Built-in extension infrastructure** (`claude-notes/plans/2026-04-01-builtin-extensions.md`):
  Completed. Built-in extensions are embedded in the binary and discovered
  before user extensions. The `resources/extensions/quarto/` directory is
  embedded via `include_dir!` and scanned by `discover_extensions()`.

- **Plan A: Lua API** (`claude-notes/plans/2026-04-01-lua-api-quarto-doc.md`):
  Must be completed first. Provides `quarto.version`,
  `quarto.base64.encode()`, `quarto.doc.is_format()`,
  `quarto.doc.add_html_dependency()`.

- **Plan B: Pipeline wiring** (`claude-notes/plans/2026-04-01-lua-api-pipeline-wiring.md`):
  Completed. Wires HTML dependencies and text includes through the
  pipeline so that `add_html_dependency()` produces `<link>` and
  `<script>` tags in the output HTML, and CSS/JS files are written to
  the output directory. Key details:
  - Artifact paths: `libs/{name}/{filename}` (relative to `{stem}_files/`)
  - Template uses `$for(header-includes)$`, `$for(include-before)$`,
    `$for(include-after)$`, `$for(scripts)$` — all as lists
  - Shared helpers in `quarto-core::dependency` module

---

## Overview

Add three built-in extensions copied from TS Quarto
(`~/src/quarto-cli/src/resources/extensions/quarto/`):

1. **`version`** — `{{< version >}}` returns Quarto version string.
   5 lines of Lua. Uses `quarto.version`.

2. **`kbd`** — `{{< kbd Shift-Ctrl-Q mac=Shift-Command-Q >}}` renders
   keyboard shortcuts as `<kbd>` tags with OS-specific variants.
   148 lines. Uses `quarto.doc.is_format()`,
   `quarto.doc.add_html_dependency()`.

3. **`placeholder`** — `{{< placeholder 300 200 >}}` generates
   placeholder images as SVG data URIs. 51 lines. Uses
   `quarto.base64.encode()`. PNG mode requires `fetch_url` which is
   not yet implemented, so placeholder defaults to SVG.

### TS Quarto source locations

| Extension | Path |
|-----------|------|
| version | `~/src/quarto-cli/src/resources/extensions/quarto/version/` |
| kbd | `~/src/quarto-cli/src/resources/extensions/quarto/kbd/` |
| placeholder | `~/src/quarto-cli/src/resources/extensions/quarto/placeholder/` |

### Lua API dependencies

| Extension | APIs Used |
|-----------|----------|
| version | `quarto.version` |
| kbd | `quarto.doc.is_format()`, `quarto.doc.isFormat()`, `quarto.doc.add_html_dependency()`, `quarto.log.warning()`, `quarto.shortcode.error_output()`, `pandoc.utils.stringify()`, `pandoc.RawInline`, `pandoc.Code`, `pandoc.Str` |
| placeholder | `quarto.base64.encode()`, `pandoc.utils.stringify()`, `pandoc.Image`, `pandoc.Str`, `pcall`, `tonumber`, `math.floor`, `math.min` |

All listed APIs are already implemented in q2. No new API work is needed.

---

## Work Items

### Phase 1: Copy extension files

- [x] **1.1** Copy `version` verbatim from TS Quarto:
  - `resources/extensions/quarto/version/_extension.yml`
  - `resources/extensions/quarto/version/version.lua`

- [x] **1.2** Copy `kbd` verbatim from TS Quarto:
  - `resources/extensions/quarto/kbd/_extension.yml`
  - `resources/extensions/quarto/kbd/kbd.lua`
  - `resources/extensions/quarto/kbd/resources/kbd.css`
  - `resources/extensions/quarto/kbd/resources/kbd.js`

- [x] **1.3** Copy `placeholder` from TS Quarto with two modifications:
  - `resources/extensions/quarto/placeholder/_extension.yml`
  - `resources/extensions/quarto/placeholder/placeholder.lua`
  - **Modification 1**: Remove the `quarto.format.is_typst_output()`
    check entirely (the function doesn't exist in q2 and Typst output
    is not supported).
  - **Modification 2**: Change the default format from `"png"` to
    `"svg"`. The original code defaults to PNG and calls an external
    service (`svg2png.deno.dev`) via `pandoc.mediabag.fetch()`, which
    doesn't work in q2 (`fetch_url` returns `NotSupported`). With SVG
    as the unconditional default, the extension works without network
    access. Users can still request PNG via `format=png` kwarg (it
    will error gracefully).
  - Net effect on the default-format logic: replace the entire
    `if quarto.format.is_typst_output() ... else ... end` block with
    `output_format = "svg"`.

### Phase 2: Verify existing APIs work end-to-end

All Lua APIs used by these extensions are already implemented in q2.
This phase just confirms they work via the smoke tests in Phase 4.

- [x] **2.1** `quarto.doc.isFormat()` — kbd.lua uses both
  `quarto.doc.is_format()` and `quarto.doc.isFormat()`. Plan A
  registers both as aliases. Verified by kbd smoke test passing.

- [x] **2.2** `quarto.shortcode.error_output()` — kbd.lua calls this
  for input validation. Already implemented in `shortcode.rs`.
  Verified by kbd smoke test passing.

- [x] **2.3** `quarto.log.warning()` — kbd.lua calls this for
  warnings. Already implemented in `quarto_api.rs`. Verified by kbd
  smoke test passing.

### Phase 3: Update WASM embedding

- [x] **3.1** The built-in extensions are embedded via `include_dir!` in
  both `crates/quarto-core/src/extension/mod.rs` (native) and
  `crates/wasm-quarto-hub-client/src/lib.rs` (WASM). Since we're adding
  files to `resources/extensions/quarto/`, both embeddings pick them up
  automatically at compile time. Verify the WASM build succeeds.

### Phase 4: Smoke tests

- [x] **4.1** `builtin-version-shortcode/test.qmd`:
  ```yaml
  _quarto:
    tests:
      html:
        noErrors: true
        ensureFileRegexMatches:
          - ["0\\.1\\.0"]
  ```
  Uses `{{< version >}}`, expects version string in output.

- [x] **4.2** `builtin-kbd-shortcode/test.qmd`:
  ```yaml
  _quarto:
    tests:
      html:
        noErrors: true
        ensureHtmlElements:
          - ["kbd"]
  ```
  Uses `{{< kbd Ctrl-C >}}`, expects `<kbd>` elements in output.
  Also verify that `kbd.css` and `kbd.js` are referenced in the HTML
  (via `ensureFileRegexMatches` for `libs/kbd/kbd.css` and
  `libs/kbd/kbd.js` in link/script tags). This is the key end-to-end
  test that the full dependency pipeline works:
  Lua `add_html_dependency` → artifact storage → template `<link>`/
  `<script>` tags → file output to `{stem}_files/libs/kbd/`.

- [x] **4.3** `builtin-placeholder-shortcode/test.qmd`:
  ```yaml
  _quarto:
    tests:
      html:
        noErrors: true
        ensureFileRegexMatches:
          - ["data:image/svg"]
  ```
  Uses `{{< placeholder 200 >}}`, expects SVG data URI in output.

### Phase 5: Verify and close Plan B items

- [x] **5.1** `cargo nextest run --workspace` — 7206 tests pass, no regressions
- [x] **5.2** WASM build + hub-client tests pass (52 tests).
  `cargo xtask verify` fails at tree-sitter step (pre-existing:
  `tree-sitter` CLI not installed), but all relevant steps pass:
  lint, format, Rust build, WASM build, hub-client tests.
- [x] **5.3** Check off Plan B items 7.1 (HTML dependency end-to-end,
  covered by kbd smoke test), 7.2 (text includes), 7.3 (is_format),
  and 7.5 (`cargo xtask verify`)

## Design Notes

### placeholder modifications from TS Quarto

TS Quarto's placeholder has two code paths we modify:

1. **Typst check removed**: The original checks
   `quarto.format.is_typst_output()` to decide the default format.
   This function doesn't exist in q2 (no Typst support), so we remove
   the entire conditional block rather than stubbing it.

2. **Default changed to SVG**: The original defaults to PNG, calling
   `pandoc.mediabag.fetch("https://svg2png.deno.dev/...")` to convert
   SVG→PNG via an external service. In q2, `fetch_url()` returns
   `NotSupported` on all platforms (no HTTP client). Rather than let
   the extension degrade to an error, we change the default to SVG.
   The PNG path remains for users who explicitly request `format=png`
   (it will error gracefully via `pcall`).

### kbd as integration test

The `kbd` extension is the simplest real-world exercise of the full
HTML dependency pipeline. It calls `add_html_dependency({name="kbd",
stylesheets={"resources/kbd.css"}, scripts={"resources/kbd.js"}})`.
This tests:
1. Path resolution (`resources/kbd.css` relative to script dir)
2. Artifact storage in the pipeline
3. Template rendering of `<link>` and `<script>` tags
4. File output to `{stem}_files/libs/kbd/`

If the kbd smoke test passes, the full pipeline is working.

Note: `kbd` also serves as Plan B's smoke test 7.1 (HTML dependency
end-to-end) — the most important integration test for the pipeline.

## Files Touched

| File | Change |
|---|---|
| `resources/extensions/quarto/version/` | New: copied from TS Quarto |
| `resources/extensions/quarto/kbd/` | New: copied from TS Quarto |
| `resources/extensions/quarto/placeholder/` | New: copied from TS Quarto (Typst check removed, SVG default) |
| Smoke test fixtures | New: version, kbd, placeholder tests |

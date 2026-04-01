# Plan: Built-in Extensions Batch 2 — version, kbd, placeholder

## Status: Not started

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
| kbd | `quarto.doc.is_format()`, `quarto.doc.isFormat()`, `quarto.doc.add_html_dependency()`, `quarto.log.warning()`, `pandoc.RawInline`, `pandoc.Code`, `pandoc.Str` |
| placeholder | `quarto.base64.encode()`, `quarto.format.is_typst_output()`, `pandoc.mediabag.fetch()`, `pandoc.Image`, `pandoc.Str`, `pcall`, `tonumber` |

---

## Work Items

### Phase 1: Copy extension files

- [ ] **1.1** Copy `version` verbatim from TS Quarto:
  - `resources/extensions/quarto/version/_extension.yml`
  - `resources/extensions/quarto/version/version.lua`

- [ ] **1.2** Copy `kbd` verbatim from TS Quarto:
  - `resources/extensions/quarto/kbd/_extension.yml`
  - `resources/extensions/quarto/kbd/kbd.lua`
  - `resources/extensions/quarto/kbd/resources/kbd.css`
  - `resources/extensions/quarto/kbd/resources/kbd.js`

- [ ] **1.3** Copy `placeholder` from TS Quarto with one modification:
  - `resources/extensions/quarto/placeholder/_extension.yml`
  - `resources/extensions/quarto/placeholder/placeholder.lua`
  - **Modification**: Change the default format from `"png"` to `"svg"`
    (lines 17-22) since `fetch_url` is not implemented on any platform.
    The original code defaults to PNG and calls an external service
    (`svg2png.deno.dev`) via `pandoc.mediabag.fetch()`. With SVG as
    default, the extension works without network access. Users can
    still request PNG via `format=png` kwarg (it will error gracefully).
  - **Note on `quarto.format.is_typst_output()`**: This function does
    not exist in q2. Since Typst output is not supported yet, the
    check can be replaced with `false` or the block removed. The net
    effect is the same: default to SVG.

### Phase 2: Handle missing APIs gracefully

- [ ] **2.1** `quarto.format.is_typst_output()` — placeholder.lua calls
  this. Options:
  (a) Add a stub `quarto.format` table with `is_typst_output()` returning
      `false`, or
  (b) Modify placeholder.lua to remove the Typst check.
  Prefer (a) since it's forward-compatible and avoids diverging from
  TS Quarto.

- [ ] **2.2** `quarto.doc.isFormat()` — kbd.lua uses both
  `quarto.doc.is_format()` and `quarto.doc.isFormat()`. Plan A
  registers both as aliases. Verify this works by running the kbd
  smoke test.

### Phase 3: Update WASM embedding

- [ ] **3.1** The built-in extensions are embedded via `include_dir!` in
  both `crates/quarto-core/src/extension/mod.rs` (native) and
  `crates/wasm-quarto-hub-client/src/lib.rs` (WASM). Since we're adding
  files to `resources/extensions/quarto/`, both embeddings pick them up
  automatically at compile time. Verify the WASM build succeeds.

### Phase 4: Smoke tests

- [ ] **4.1** `builtin-version-shortcode/test.qmd`:
  ```yaml
  _quarto:
    tests:
      html:
        noErrors: true
        ensureFileRegexMatches:
          - ["0\\.1\\.0"]
  ```
  Uses `{{< version >}}`, expects version string in output.

- [ ] **4.2** `builtin-kbd-shortcode/test.qmd`:
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

- [ ] **4.3** `builtin-placeholder-shortcode/test.qmd`:
  ```yaml
  _quarto:
    tests:
      html:
        noErrors: true
        ensureFileRegexMatches:
          - ["data:image/svg"]
  ```
  Uses `{{< placeholder 200 >}}`, expects SVG data URI in output.

### Phase 5: Verify

- [ ] **5.1** `cargo nextest run --workspace` — no regressions
- [ ] **5.2** `cargo xtask verify` — full verification including WASM

## Design Notes

### placeholder PNG mode

TS Quarto's placeholder defaults to PNG by calling
`pandoc.mediabag.fetch("https://svg2png.deno.dev/...")`. This external
service converts SVG to PNG. In q2, `SystemRuntime::fetch_url()` is
defined in the trait but returns `NotSupported` on all platforms (no
HTTP client wired up yet). The `mediabag.fetch` Lua function calls
`fetch_url` for URLs, which returns `(nil, nil)` on failure.
`placeholder.lua` wraps this in `pcall`, so it degrades to an error
string rather than crashing. We change the default to SVG to avoid
this degraded experience.

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
| `resources/extensions/quarto/placeholder/` | New: copied from TS Quarto (SVG default) |
| `crates/pampa/src/lua/quarto_api.rs` | Add `quarto.format.is_typst_output()` stub |
| Smoke test fixtures | New: version, kbd, placeholder tests |

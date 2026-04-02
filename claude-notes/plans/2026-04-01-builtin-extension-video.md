# Plan: Built-in Extension — video

## Status: Complete

## Prerequisites

- **Plan A: Lua API** (`claude-notes/plans/2026-04-01-lua-api-quarto-doc.md`):
  Complete. Provides `quarto.doc.is_format()`,
  `quarto.doc.add_html_dependency()`, `quarto.doc.include_text()`,
  `quarto.doc.has_bootstrap()`, and `dofile`/`loadfile` overrides
  backed by `SystemRuntime` in the restricted (WASM/test) environment.

- **Plan B: Pipeline wiring** (`claude-notes/plans/2026-04-01-lua-api-pipeline-wiring.md`):
  Complete (except 7.2 text-includes smoke test — this extension can
  serve as that test). Wires HTML dependencies, text includes, and
  `PandocIncludes` through the pipeline and template so that
  `add_html_dependency()` produces `<link>`/`<script>` tags and
  `include_text()` injects raw HTML at the correct locations.
  Key module: `crate::dependency` has shared helpers
  `store_html_dependencies()` and `push_text_includes()`.
  Artifact paths use `libs/{name}/{filename}` convention matching Quarto 1.

- **Built-in extension infrastructure** (`claude-notes/plans/2026-04-01-builtin-extensions.md`):
  Complete. Built-in extensions are embedded and discovered.

- **Batch 2 extensions** (`claude-notes/plans/2026-04-01-builtin-extensions-batch2.md`):
  Complete. version, kbd, placeholder extensions are working. The kbd
  extension confirmed the full HTML dependency pipeline works end-to-end
  (add_html_dependency → artifacts → template tags → file output).
  All Lua APIs listed below are confirmed working.

---

## Overview

Add the `video` built-in extension from TS Quarto. This is more complex
than the batch 2 extensions because it includes both a **shortcode**
(`video.lua`, 368 lines) and a companion **filter** (`video-filter.lua`,
24 lines). The filter uses `dofile()` to load shared code from the
shortcode file.

### What `video` does

`{{< video https://www.youtube.com/watch?v=dQw4w9WgXcQ >}}` embeds
videos from YouTube, Vimeo, Brightcove, or local files (via VideoJS).

- **HTML output**: Generates `<iframe>` (YouTube/Vimeo/Brightcove) or
  `<video>` (VideoJS) wrapped in responsive `<div>`. Registers CSS/JS
  dependencies for VideoJS and responsive styles.
- **AsciiDoc output**: Uses AsciiDoc video macro syntax.
- **Markdown/other output**: Falls back to links or image syntax.

### Lua API dependencies

All APIs confirmed working in batch 2 testing except `quarto.utils.as_inlines()`
(not yet implemented — see Phase 4.1).

| API | Status | Used for |
|-----|--------|----------|
| `quarto.doc.is_format()` | Working | Format-specific output generation |
| `quarto.doc.add_html_dependency()` | Working | VideoJS CSS/JS, Bootstrap responsive CSS |
| `quarto.doc.include_text("after-body", ...)` | Working | VideoJS initialization scripts |
| `quarto.doc.has_bootstrap()` | Working | Responsive aspect ratio classes |
| `quarto.utils.resolve_path()` | Working | Path resolution |
| `pandoc.RawBlock`, `pandoc.Link`, `pandoc.Image` | Working | AST construction |
| `pandoc.utils.stringify()` | Working | String conversion |
| `quarto.shortcode.error_output()` | Working | Error handling |
| `dofile()` | Working | `video-filter.lua` loads shared code from `video.lua` |
| `quarto.utils.as_inlines()` | **Missing** | Needs implementation (see Phase 4.1) |

### TS Quarto source

`~/src/quarto-cli/src/resources/extensions/quarto/video/`:
- `_extension.yml` — contributes shortcode + filter
- `video.lua` — main shortcode handler (368 lines)
- `video-filter.lua` — companion filter for `background-video` in
  reveal.js headers (24 lines)
- `resources/videojs/video.min.js`, `resources/videojs/video-js.css` — VideoJS player library
- `resources/bootstrap/bootstrap-responsive-ratio.css` — responsive video wrapper CSS

### `dofile()` in `video-filter.lua`

Line 1 of `video-filter.lua`:
```lua
local videoHelpers = dofile(quarto.utils.resolve_path('video.lua'))
```

This loads the shortcode file as a module to reuse its URL parsing
functions. Plan A overwrites `dofile` in the restricted Lua environment
to route through `SystemRuntime`, making this work on all platforms
(WASM, tests, native).

### How extension filters are activated (TS Quarto behavior)

In TS Quarto, **top-level `contributes.filters`** are NOT automatically
loaded into the pipeline. Users must explicitly add the extension name
to their document's `filters:` metadata (e.g., `filters: [video]`) for
the filter to run. Only **format extension filters** (declared under
`contributes.formats.<format>.filters`) are auto-injected via metadata
merge.

The video extension's `_extension.yml` declares:
```yaml
contributes:
  shortcodes:
    - video.lua
  filters:
    - video-filter.lua
```

The shortcode is auto-discovered (built-in extension infrastructure
handles this). The filter only runs if a user explicitly adds
`filters: [video]` to their document. Since `video-filter.lua` only
handles reveal.js `background-video` attributes (which q2 doesn't
support yet), this filter is effectively dead code for now. We copy it
for completeness — zero cost, 24 lines — and our existing
`filter_resolve.rs` already handles the case where a user references
an extension by name in `filters:`.

---

## Work Items

### Phase 1: Copy extension files

- [x] **1.1** Copy `video` from TS Quarto:
  - `resources/extensions/quarto/video/_extension.yml`
  - `resources/extensions/quarto/video/video.lua`
  - `resources/extensions/quarto/video/video-filter.lua`

- [x] **1.2** Copy VideoJS resource files from TS Quarto. These are
  bundled (not CDN-loaded):
  - `resources/extensions/quarto/video/resources/videojs/video.min.js`
  - `resources/extensions/quarto/video/resources/videojs/video-js.css`
  - `resources/extensions/quarto/video/resources/bootstrap/bootstrap-responsive-ratio.css`

- [x] **1.3** Verify `_extension.yml` is read correctly by our extension
  infrastructure. It contributes `shortcodes: [video.lua]` and
  `filters: [video-filter.lua]`. No new infrastructure needed — the
  shortcode is auto-discovered, and the filter is only activated by
  explicit user reference (matching TS Quarto behavior). Our existing
  `filter_resolve.rs:try_resolve_extension_filter()` handles this.

### Phase 2: Verify `dofile` works for built-in extensions

- [x] **2.1** The `dofile(quarto.utils.resolve_path('video.lua'))` call
  in `video-filter.lua` resolves the path relative to the filter's
  script directory. For built-in extensions, this is the extracted temp
  dir (native) or VFS path (WASM). Verify that `resolve_path` correctly
  resolves sibling files within the same extension directory.

- [x] **2.2** Write a unit test: a filter that uses `dofile()` to load
  a sibling Lua file, verifying it works in the test (restricted)
  environment.

### Phase 3: Handle missing APIs

- [x] **3.1** Implement `quarto.utils.as_inlines()`. This is a **type
  coercion function** (not a markdown parser). It converts various Pandoc
  AST types to `pandoc.Inlines`:
  - Inlines → return as-is
  - Single Inline → wrap in Inlines list
  - Blocks/Block → `pandoc.utils.blocks_to_inlines()`
  - List/table of Inlines → set Inlines metatable directly
  - Anything else (including strings, nil) → `pandoc.Inlines(obj or {})`

  Reference implementation: `~/src/quarto-cli/src/resources/pandoc/datadir/_utils.lua:280-300`.

  In video.lua specifically, `as_inlines(titleValue)` receives a string
  (from `pandoc.utils.stringify()`), hitting the final `else` branch →
  `pandoc.Inlines(titleValue)`. Our `pandoc.Image` constructor already
  handles strings via `peek_inlines_fuzzy`, so video.lua would work
  without `as_inlines` in practice, but we implement it for compatibility
  with other extensions that may pass Blocks or tables.

- [x] **3.2** Review all Pandoc constructors used by video.lua:
  `pandoc.RawBlock`, `pandoc.Link`, `pandoc.Image`. These should
  already exist from the constructor work. Verify.

### Phase 4: Smoke tests

Smoke test assertions use the two-array format documented in
`claude-notes/instructions/testing.md:115-136`:
- First array element: patterns that **must match**
- Second array element (optional): patterns that **must NOT match**
- All must-match patterns go in a **single array** (the first element).

- [x] **4.1** `builtin-video-youtube/test.qmd`:
  ```yaml
  _quarto:
    tests:
      html:
        noErrors: true
        ensureFileRegexMatches:
          - ["youtube.com/embed", "quarto-video"]
  ```
  Uses `{{< video https://www.youtube.com/watch?v=dQw4w9WgXcQ >}}`.
  Expects YouTube iframe and wrapper div in output.

- [x] **4.2** `builtin-video-local/test.qmd` (if feasible):
  Test local video with VideoJS. Verify VideoJS dependency files are
  included (CSS/JS `<link>`/`<script>` tags in output, files written
  to output directory). This exercises `add_html_dependency` +
  `include_text("after-body", ...)` together. May need a dummy video
  file or skip if too complex.

### Phase 5: Verify

- [x] **5.1** `cargo nextest run --workspace` — no regressions
- [x] **5.2** `cargo xtask verify` — full verification including WASM
  (tree-sitter-cli is now in dev-setup, so all steps should pass)
- [x] **5.3** Check off Plan B item 7.2 (text includes smoke test),
  since the video extension exercises `include_text("after-body", ...)`

## Design Notes

### Filter activation model (matching TS Quarto)

In TS Quarto, there are two ways extensions contribute filters:
1. **Top-level `contributes.filters`**: Not auto-loaded. User must add
   extension name to `filters:` metadata. Resolved at render time by
   `resolveFilterExtension()` (TS) / `try_resolve_extension_filter()` (q2).
2. **Format `contributes.formats.<fmt>.filters`**: Auto-injected via
   metadata merge during format resolution.

The video extension uses approach 1. Our `filter_resolve.rs` already
supports this — no new infrastructure needed.

### `dofile` sharing pattern

`video-filter.lua` reuses code from `video.lua` via `dofile`. This is
a common pattern in TS Quarto extensions — a filter loads the shortcode
file as a library. Plan A's `dofile` overwrite (routing through
`SystemRuntime` in restricted env) makes this work transparently.

### VideoJS dependency chain

When a local/self-hosted video is embedded, video.lua:
1. Calls `quarto.doc.add_html_dependency()` with VideoJS CSS/JS files
2. Calls `quarto.doc.include_text("after-body", ...)` with a `<script>`
   tag that initializes VideoJS on the video element

This exercises both the dependency pipeline (Plan B: artifacts with
`libs/{name}/{filename}` paths → template `<link>`/`<script>` tags →
file output to `{stem}_files/libs/{name}/`) and the text-include
pipeline (Plan B: `PandocIncludes` → template `$for(include-after)$`),
making `video` a good end-to-end test for the full infrastructure.

Note: `video` also serves as Plan B's smoke test 7.2 (text includes)
since it exercises `include_text("after-body", ...)`.

## Files Touched

| File | Change |
|---|---|
| `resources/extensions/quarto/video/` | New: copied from TS Quarto (Lua files + resource files) |
| Smoke test fixtures | New: video shortcode tests |
| `crates/pampa/src/lua/quarto_api.rs` | Add `quarto.utils.as_inlines()` |

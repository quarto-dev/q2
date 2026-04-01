# Plan: Built-in Extension — video

## Status: Not started

## Prerequisites

- **Plan A: Lua API** (`claude-notes/plans/2026-04-01-lua-api-quarto-doc.md`):
  Must be completed first. Provides `quarto.doc.is_format()`,
  `quarto.doc.add_html_dependency()`, `quarto.doc.include_text()`,
  `quarto.doc.has_bootstrap()`, and `dofile`/`loadfile` overrides
  backed by `SystemRuntime` in the restricted (WASM/test) environment.

- **Plan B: Pipeline wiring** (`claude-notes/plans/2026-04-01-lua-api-pipeline-wiring.md`):
  Completed. Wires HTML dependencies, text includes, and
  `PandocIncludes` through the pipeline and template so that
  `add_html_dependency()` produces `<link>`/`<script>` tags and
  `include_text()` injects raw HTML at the correct locations.
  Key module: `crate::dependency` has shared helpers
  `store_html_dependencies()` and `push_text_includes()`.
  Artifact paths use `libs/{name}/{filename}` convention matching Quarto 1.

- **Built-in extension infrastructure** (`claude-notes/plans/2026-04-01-builtin-extensions.md`):
  Completed. Built-in extensions are embedded and discovered.

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

| API | Used for |
|-----|----------|
| `quarto.doc.is_format()` | Format-specific output generation |
| `quarto.doc.add_html_dependency()` | VideoJS CSS/JS, Bootstrap responsive CSS |
| `quarto.doc.include_text("after-body", ...)` | VideoJS initialization scripts |
| `quarto.doc.has_bootstrap()` | Responsive aspect ratio classes |
| `quarto.utils.resolve_path()` | Path resolution (already implemented) |
| `pandoc.RawBlock`, `pandoc.Link`, `pandoc.Image` | AST construction |
| `dofile()` | `video-filter.lua` loads shared code from `video.lua` |

### TS Quarto source

`~/src/quarto-cli/src/resources/extensions/quarto/video/`:
- `_extension.yml` — contributes shortcode + filter
- `video.lua` — main shortcode handler (368 lines)
- `video-filter.lua` — companion filter for `background-video` in
  reveal.js headers (24 lines)
- `resources/videojs/` — VideoJS library CSS/JS files (if bundled)

### `dofile()` in `video-filter.lua`

Line 1 of `video-filter.lua`:
```lua
local videoHelpers = dofile(quarto.utils.resolve_path('video.lua'))
```

This loads the shortcode file as a module to reuse its URL parsing
functions. Plan A overwrites `dofile` in the restricted Lua environment
to route through `SystemRuntime`, making this work on all platforms
(WASM, tests, native).

---

## Work Items

### Phase 1: Copy extension files

- [ ] **1.1** Copy `video` from TS Quarto:
  - `resources/extensions/quarto/video/_extension.yml`
  - `resources/extensions/quarto/video/video.lua`
  - `resources/extensions/quarto/video/video-filter.lua`

- [ ] **1.2** Check if VideoJS library files are bundled in TS Quarto's
  `video/resources/` directory. If so, copy them. If VideoJS is loaded
  from a CDN in the Lua code, no additional files needed.

- [ ] **1.3** Review `_extension.yml` to understand what it contributes:
  likely `shortcodes: [video.lua]` and `filters: [video-filter.lua]`.
  Verify our filter resolution infrastructure can handle a built-in
  extension that contributes both shortcodes and filters.

### Phase 2: Verify `dofile` works for built-in extensions

- [ ] **2.1** The `dofile(quarto.utils.resolve_path('video.lua'))` call
  in `video-filter.lua` resolves the path relative to the filter's
  script directory. For built-in extensions, this is the extracted temp
  dir (native) or VFS path (WASM). Verify that `resolve_path` correctly
  resolves sibling files within the same extension directory.

- [ ] **2.2** Write a unit test: a filter that uses `dofile()` to load
  a sibling Lua file, verifying it works in the test (restricted)
  environment.

### Phase 3: Verify filter infrastructure for built-in extensions

- [ ] **3.1** Check that the filter resolution code
  (`crates/quarto-core/src/filter_resolve.rs`) can find filters
  contributed by built-in extensions. The extension's `contributes.filters`
  contains relative paths (e.g., `video-filter.lua`). These need to
  resolve to the extension's directory on disk (temp dir or VFS path).

- [ ] **3.2** If filters from built-in extensions aren't automatically
  discovered (since they're not in the document's `filters:` metadata),
  determine how TS Quarto triggers them. Built-in extension filters may
  need explicit metadata in `_extension.yml` under
  `contributes.formats.html.filters`.

### Phase 4: Handle missing APIs

- [ ] **4.1** `quarto.utils.as_inlines()` — video.lua uses this. Check
  if it exists in q2. If not, add it (likely a thin wrapper around
  `pandoc.Inlines()`).

- [ ] **4.2** Review all Pandoc constructors used by video.lua:
  `pandoc.RawBlock`, `pandoc.Link`, `pandoc.Image`. These should
  already exist from the constructor work. Verify.

### Phase 5: Smoke tests

- [ ] **5.1** `builtin-video-youtube/test.qmd`:
  ```yaml
  _quarto:
    tests:
      html:
        noErrors: true
        ensureFileRegexMatches:
          - ["youtube.com/embed"]
          - ["quarto-video"]
  ```
  Uses `{{< video https://www.youtube.com/watch?v=dQw4w9WgXcQ >}}`.
  Expects YouTube iframe and wrapper div in output.

- [ ] **5.2** `builtin-video-local/test.qmd` (if feasible):
  Test local video with VideoJS. Verify VideoJS dependency files are
  included (CSS/JS `<link>`/`<script>` tags in output, files written
  to output directory). This exercises `add_html_dependency` +
  `include_text("after-body", ...)` together. May need a dummy video
  file or skip if too complex.

### Phase 6: Verify

- [ ] **6.1** `cargo nextest run --workspace` — no regressions
- [ ] **6.2** `cargo xtask verify` — full verification including WASM

## Design Notes

### Filter vs shortcode extension

Most built-in extensions only contribute shortcodes. `video` is unique
in contributing both a shortcode AND a filter. The shortcode handles
`{{< video >}}` syntax. The filter handles `background-video` attributes
on reveal.js slide headers. The filter is only relevant for reveal.js
output, which we don't support yet, but we should include it for
completeness.

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
| `resources/extensions/quarto/video/` | New: copied from TS Quarto |
| Smoke test fixtures | New: video shortcode tests |
| Possibly `crates/pampa/src/lua/quarto_api.rs` | Add `quarto.utils.as_inlines()` if missing |

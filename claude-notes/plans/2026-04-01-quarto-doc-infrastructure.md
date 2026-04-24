# Plan: `quarto.doc` Lua API + HTML Dependency Infrastructure

## Status: Complete

Landed on `main` as:
- `446742b6` "Add quarto.doc Lua API, script-dir stack, and dofile/loadfile for WASM" (pre-squash)
- `f87b11b9` "Add built-in extension infrastructure, Lua APIs, and pipeline wiring" (squashed landing on main, 2026-04-01)
- `1e2cb0a3` "Remove script-dir push/pop from WASM dofile override" (cleanup, 2026-04-16)
- `52968801` "Remove test arm from wasm32 cfg guards in Lua initialization" (cleanup, 2026-04-16)
- `chore/incomplete-plans` (this branch) — Phase 5 smoke fixture `quarto-doc-api-extension`

---

## Overview

Add the `quarto.doc` Lua namespace and HTML dependency infrastructure so
that Lua shortcode extensions can register CSS/JS dependencies, detect
the output format, inject text at specific document locations, and check
for Bootstrap. Also add `quarto.version` and `quarto.base64.encode()`.

This is the foundation plan. Two follow-up plans add built-in extensions
that exercise these APIs:
- `claude-notes/plans/2026-04-01-builtin-extensions-batch2.md` (version, kbd, placeholder)
- `claude-notes/plans/2026-04-01-builtin-extension-video.md` (video)

## Background

### What TS Quarto provides

TS Quarto's `quarto.doc` namespace (defined in `init.lua`) provides:
- `is_format(fmt)` — check output format
- `add_html_dependency({name, stylesheets, scripts, ...})` — register
  CSS/JS deps, deduplicated by name
- `include_text(location, text)` — inject raw text at
  `"in-header"` / `"before-body"` / `"after-body"`
- `has_bootstrap()` — check if Bootstrap is in use

TS Quarto communicates dependencies via a temp file (JSON Lines) because
Pandoc is an external process. We don't need that — our Lua runs
in-process, so we use the same pattern as diagnostics: store in a Lua
table, extract after execution, push onto `StageContext`.

### Current q2 state

- **No `quarto.doc` namespace** exists
- **No `quarto.version`** — only `PANDOC_VERSION` is set
- **No `quarto.base64`** — base64 exists internally in mediabag but not
  exposed
- **Diagnostics pattern**: `quarto._diagnostics` Lua table, extracted
  via `extract_lua_diagnostics()` after filter execution. Shortcode
  execution does NOT currently extract Lua diagnostics (pre-existing gap)
- **Artifact store**: `StageContext.artifacts` is a key-value store with
  `get_by_prefix()`. CSS compilation already uses it (`"css:default"`
  artifact). WASM post-pipeline loop writes all artifacts with paths to
  VFS automatically.
- **`FORMAT` global**: Already set in `LuaShortcodeEngine::new()` to the
  target format string (e.g. `"html"`)
- **`PandocIncludes`**: Exists as a struct with `header_includes`,
  `include_before`, `include_after` but only used by knitr engine, not
  exposed to Lua
- **`dofile()`/`loadfile()`**: Available from mlua's base library
  (always loaded) but use C `fopen` directly, bypassing `SystemRuntime`.
  In WASM, `fopen` returns null so they fail. Need to overwrite with
  `SystemRuntime`-backed versions.

### Key files

| File | Role |
|---|---|
| `crates/pampa/src/lua/quarto_api.rs` | Registers `quarto.*` Lua namespace |
| `crates/pampa/src/lua/shortcode.rs` | `LuaShortcodeEngine` — sets up globals, calls handlers |
| `crates/pampa/src/lua/filter.rs` | Filter execution — restricted Lua setup |
| `crates/pampa/src/lua/diagnostics.rs` | Diagnostic collection pattern (model for deps) |
| `crates/pampa/src/lua/io_wasm.rs` | Synthetic `io.open` for restricted env |
| `crates/pampa/src/lua/os_wasm.rs` | Synthetic `os.*` for restricted env |
| `crates/pampa/src/lua/mediabag.rs` | `pandoc.mediabag` including `make_data_uri` |
| `crates/quarto-core/src/stage/context.rs` | `StageContext` — shared pipeline context |
| `crates/quarto-core/src/artifact.rs` | `ArtifactStore` with `get_by_prefix()` |
| `crates/quarto-core/src/stage/stages/apply_template.rs` | Template stage — consumes CSS artifacts |
| `crates/quarto-core/src/stage/stages/compile_theme_css.rs` | Stores `"css:default"` artifact |
| `crates/quarto-core/src/transforms/shortcode_resolve.rs` | Shortcode transform — calls pampa Lua engine |
| `crates/quarto-core/src/render_to_file.rs` | Native file output — writes artifacts to disk |
| `crates/wasm-quarto-hub-client/src/lib.rs` | WASM — writes artifacts to VFS post-pipeline |
| `crates/quarto-core/src/stage/data.rs` | `PandocIncludes` struct |

### Architecture: How data flows

The pipeline runs stages sequentially, all sharing one `&mut StageContext`:

```
ParseDocument → EngineExecution → MetadataMerge → CompileThemeCss →
  UserFilters(pre) → AstTransforms → UserFilters(post) →
  RenderHtmlBody → ApplyTemplate
```

**AstTransforms** runs shortcodes (via `ShortcodeResolveTransform`). A
Lua shortcode calls `quarto.doc.add_html_dependency(...)` which stores
data in a Lua table. After execution, Rust extracts deps and stores
them as artifacts on `StageContext`.

**ApplyTemplate** runs later, reads all `css:*` and `js:*` artifacts,
generates `<link>` and `<script>` tags in the HTML template.

**Post-pipeline**: Native `render_to_file` writes artifact files to
`{stem}_files/`. WASM's existing loop writes all artifacts with paths
to VFS.

### Diagnostic collection pattern (model for HTML deps)

Diagnostics use Lua-table storage, not `Arc<Mutex>`:
1. `register_quarto_namespace()` creates `quarto._diagnostics` table
2. `quarto.warn()` / `quarto.error()` push entries onto it
3. `extract_lua_diagnostics(&lua)` reads them back after execution
4. Filter returns `Vec<DiagnosticMessage>` alongside the AST
5. Transform pushes onto `ctx.add_diagnostic()`

HTML dependencies will follow the same pattern with
`quarto.doc._dependencies` and `quarto.doc._text_includes`.

---

## Work Items

### Phase 1: `quarto.version` and `quarto.base64`

- [x] **1.1** Add `quarto.version` to the Lua environment. In
  `quarto_api.rs`, set `quarto.version` to a Lua table `{0, 1, 0}` (or
  whatever our current version is). TS Quarto uses a list that gets
  `table.concat(quarto.version, '.')` to produce `"1.4.0"`.

- [x] **1.2** Add `quarto.base64` namespace with `encode(data)`. The
  base64 encoding already exists in `mediabag.rs` (`make_data_uri` uses
  it internally via the `base64` crate). Expose it as
  `quarto.base64.encode(string) -> string`.

- [x] **1.3** Unit tests in pampa for both APIs.

### Phase 2: Overwrite `dofile`/`loadfile` in restricted Lua

- [x] **2.1** In the restricted Lua setup (both `filter.rs` and
  `shortcode.rs`, the `#[cfg(any(target_arch = "wasm32", test))]`
  path), after registering synthetic io/os, overwrite `dofile` and
  `loadfile` globals with Rust functions that:
  - Read the file via `runtime.file_read()`
  - Compile via `lua.load(content).set_name(filename)`
  - For `dofile`: execute immediately and return results
  - For `loadfile`: return the compiled chunk without executing

  This ensures all file access goes through `SystemRuntime`, making
  `dofile` work in WASM (where C `fopen` returns null).

  **Update:** `52968801` later removed the `test` arm per `.claude/rules/wasm.md`; the cfg is now `#[cfg(target_arch = "wasm32")]`.

- [x] **2.2** Unit tests: `dofile` and `loadfile` work in the test
  (restricted) environment with temp files via `NativeRuntime`.

### Phase 3: `quarto.doc` Lua namespace

- [x] **3.1** Create `crates/pampa/src/lua/quarto_doc.rs` with:

  ```rust
  pub fn register_quarto_doc(lua: &Lua, format: &str) -> Result<()>
  ```

  This registers the `quarto.doc` table on the existing `quarto` global:
  - `quarto.doc.is_format(fmt)` — compares `fmt` against the `FORMAT`
    global (already set). Support both exact match (`"html"`) and
    prefix match (`"html:js"` matches `"html"`), matching TS Quarto's
    `quarto.doc.is_format()` behavior.
  - `quarto.doc.has_bootstrap()` — for now, return `true` when format
    is HTML (we always use Bootstrap themes in HTML output). Can be
    refined later when we support non-Bootstrap HTML.
  - `quarto.doc._dependencies` — internal Lua table for collecting deps
  - `quarto.doc._text_includes` — internal Lua table for text injections
  - `quarto.doc.add_html_dependency(dep)` — validates `dep.name`
    (required), `dep.stylesheets` and `dep.scripts` (optional arrays
    of strings). Resolves relative paths via `quarto.utils.resolve_path`.
    Deduplicates by name (skip if name already in table). Pushes onto
    `quarto.doc._dependencies`.
  - `quarto.doc.include_text(location, text)` — validates `location`
    is one of `"in-header"`, `"before-body"`, `"after-body"`.
    Pushes `{location, text}` onto `quarto.doc._text_includes`.

  **Implementation note:** actual signature is `register_quarto_doc(lua: &Lua)` — the `is_format` closure reads the `FORMAT` global at call time rather than taking it as an argument. camelCase aliases (`isFormat`, `addHtmlDependency`, etc.) also registered for TS Quarto compat.

- [x] **3.2** Create extraction functions:
  ```rust
  pub fn extract_html_dependencies(lua: &Lua) -> Result<Vec<HtmlDependency>>
  pub fn extract_text_includes(lua: &Lua) -> Result<Vec<TextInclude>>
  ```

  Where:
  ```rust
  pub struct HtmlDependency {
      pub name: String,
      pub stylesheets: Vec<PathBuf>,  // resolved absolute paths
      pub scripts: Vec<PathBuf>,      // resolved absolute paths
  }

  pub struct TextInclude {
      pub location: IncludeLocation,  // InHeader, BeforeBody, AfterBody
      pub content: String,
  }

  pub enum IncludeLocation {
      InHeader,
      BeforeBody,
      AfterBody,
  }
  ```

- [x] **3.3** Call `register_quarto_doc()` in both `shortcode.rs`
  `LuaShortcodeEngine::new()` and `filter.rs` `apply_lua_filter()`,
  passing the target format string.

- [x] **3.4** Unit tests in pampa for all `quarto.doc.*` functions.

### Phase 4: Wire dependencies into the pipeline

- [x] **4.1** Add `html_dependencies: Vec<HtmlDependency>` and
  `text_includes: Vec<TextInclude>` fields to `StageContext`.

  **Deviation:** no new `StageContext` fields were needed. Dependencies flow through the existing `artifacts` store (`css:*`, `js:*`) and text includes flow through the existing `ctx.includes: PandocIncludes`.

- [x] **4.2** In `shortcode_resolve.rs`, after shortcode execution,
  call `extract_html_dependencies()` and `extract_text_includes()` on
  the Lua engine. Store results on `StageContext` (via `RenderContext`
  which has access to the artifact store).

  Also fix the pre-existing gap: call `extract_lua_diagnostics()` on
  the shortcode engine's Lua state and push onto `ctx.diagnostics`.

  Wired at `shortcode_resolve.rs:578-599` and mirrored in `user_filters.rs:179-185` so filter extensions get the same treatment.

- [x] **4.3** For each `HtmlDependency`, read file contents via
  `runtime.file_read()` and store as artifacts:
  - `"css:{name}"` for each stylesheet, with path like
    `{name}/{filename}` relative to the output resource dir
  - `"js:{name}"` for each script, same path convention

  **Deviation:** artifact keys are `css:{name}:{filename}` / `js:{name}:{filename}` and paths are `libs/{name}/{filename}` (matching TS Quarto's `libs/` convention). See `dependency.rs:store_html_dependencies`.

- [x] **4.4** For each `TextInclude`, store as artifacts with keys like
  `"include:in-header:{n}"`, `"include:before-body:{n}"`,
  `"include:after-body:{n}"`.

  **Deviation:** text includes do not go through the artifact store. They're pushed directly onto `ctx.includes: PandocIncludes` (`header_includes` / `include_before` / `include_after`), which the template already renders via `$for(header-includes)$` etc. Simpler and reuses the engine-execution plumbing.

- [x] **4.5** In `ApplyTemplateStage`, collect artifacts:
  - All `"css:*"` artifacts → add their paths to the CSS list alongside
    the existing `"css:default"` path
  - All `"js:*"` artifacts → generate `<script src="...">` tags (new)
  - All `"include:in-header:*"` → inject into `<head>`
  - All `"include:before-body:*"` → inject before `<body>` content
  - All `"include:after-body:*"` → inject after `<body>` content

  CSS/JS collection at `apply_template.rs:166-182` (prepends the `{stem}_files/` resource prefix). Text includes flow through the existing `PandocIncludes` template bindings — see 4.4 deviation.

- [x] **4.6** In `render_to_file.rs`, after the existing CSS write,
  add a loop that writes all `"css:*"` and `"js:*"` artifacts to
  `{stem}_files/`. (WASM already handles this — the existing
  artifact→VFS loop writes all artifacts with paths.)

  **Deviation:** no new loop needed — the existing generic artifact-writing code handles the new `css:*`/`js:*` artifacts uniformly.

### Phase 5: Integration tests

- [x] **5.1** Create a smoke test with a custom extension that uses
  `quarto.doc.add_html_dependency()` to register a CSS file. Verify
  the CSS file content appears in the output (either as a `<link>` tag
  pointing to the right path, or inline for WASM).

- [x] **5.2** Create a smoke test that uses `quarto.doc.include_text()`
  to inject content at `"after-body"`. Verify it appears in the output.

- [x] **5.3** Create a smoke test that uses `quarto.doc.is_format()` to
  produce format-dependent output.

  5.1–5.3 are covered by a single fixture: `crates/quarto/tests/smoke-all/extensions/quarto-doc-api-extension/` exercises `add_html_dependency` (stylesheet + file-exists check), `include_text` at both `in-header` and `after-body`, and `is_format('html')` in one shortcode.

- [x] **5.4** `cargo nextest run --workspace` — no regressions

- [x] **5.5** `cargo xtask verify` — full verification including WASM

## Design Notes

### Why use the artifact store for dependencies

The artifact store (`StageContext.artifacts`) already handles the
CSS compilation → template → file output flow. The pattern:
1. Stage stores content as artifact with a path
2. Template stage references the path in HTML tags
3. Post-pipeline code writes content to the path on disk (native)
   or VFS (WASM)

HTML dependencies follow the same pattern. The `get_by_prefix("css:")`
API was designed for exactly this kind of batch collection.

### Deduplication

HTML dependencies are deduplicated by `name` at registration time in
Lua (matching TS Quarto's behavior). If a shortcode is called 5 times,
the dependency is registered once.

### Path resolution

When Lua calls `add_html_dependency({stylesheets={"kbd.css"}})`, the
path `"kbd.css"` is relative to the extension directory (where the
Lua script lives). `quarto.utils.resolve_path()` already resolves
relative to `_quarto_script_dir`. The Rust extraction converts these
to absolute paths.

### No `Arc<Mutex>` needed

The diagnostic pattern shows the way: Lua stores data in tables,
Rust extracts it synchronously after execution. The pipeline is
single-threaded through each stage. No shared mutable state needed.

### `dofile`/`loadfile` overwrite rationale

mlua unconditionally loads the C base library (`luaopen_base`),
which includes `dofile` and `loadfile`. These call C `fopen` directly,
bypassing `SystemRuntime`. In WASM, `fopen` returns null (from
`c_shim.rs`), so they silently fail. In tests, they use real `fopen`,
bypassing the synthetic io layer.

Overwriting them after Lua state creation (same pattern as io/os
replacement) ensures all file access goes through `SystemRuntime`.
This is not shadowing (as in JavaScript prototype chains) — Lua
globals are a flat table, so setting `_G["dofile"]` replaces the
old function entirely.

## Files Touched

| File | Change |
|---|---|
| `crates/pampa/src/lua/quarto_api.rs` | Add `quarto.version`, `quarto.base64` |
| `crates/pampa/src/lua/quarto_doc.rs` | New: `quarto.doc` namespace registration |
| `crates/pampa/src/lua/mod.rs` | Add `quarto_doc` module |
| `crates/pampa/src/lua/shortcode.rs` | Call `register_quarto_doc`, extract deps after execution |
| `crates/pampa/src/lua/filter.rs` | Call `register_quarto_doc`, overwrite `dofile`/`loadfile` |
| `crates/quarto-core/src/stage/context.rs` | Add `html_dependencies`, `text_includes` fields |
| `crates/quarto-core/src/transforms/shortcode_resolve.rs` | Extract deps + diagnostics from Lua |
| `crates/quarto-core/src/stage/stages/apply_template.rs` | Collect `css:*`, `js:*`, `include:*` artifacts |
| `crates/quarto-core/src/render_to_file.rs` | Write `css:*`, `js:*` artifacts to output dir |
| Smoke test fixtures | New: tests for `add_html_dependency`, `include_text`, `is_format` |

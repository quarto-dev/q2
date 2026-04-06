# Plan B: Pipeline Wiring + Template (quarto-core crate)

## Status: Complete (+ Phase 8 pullback for WASM script safety)

## Prerequisites

- **Plan A** (`2026-04-01-lua-api-quarto-doc.md`): Must be completed
  first. Provides the `quarto.doc` Lua namespace, extraction functions
  (`extract_html_dependencies`, `extract_text_includes`), extraction
  methods on `LuaShortcodeEngine`, and the script-dir stack.

  Plan A leaves the return type of `apply_lua_filter` / `apply_lua_filters`
  unchanged — extraction is called but results discarded (marked with
  `// TODO(plan-b)` at `filter.rs:213`). Plan A already exports
  `HtmlDependency`, `TextInclude`, `IncludeLocation`,
  `extract_html_dependencies`, and `extract_text_includes` from
  `lua/mod.rs`. Plan B introduces a `FilterOutput` struct, widens
  return types, and wires everything into the pipeline.

---

## Overview

Wire the Lua API from Plan A into the quarto-core rendering pipeline
so that HTML dependencies and text includes actually reach the final
HTML output. Also fix two pre-existing gaps: the engine execution stage
discarding `PandocIncludes` from knitr, and shortcode execution not
extracting diagnostics.

### How TS Quarto does this (for context)

TS Quarto uses a two-stage approach because Pandoc is an external
process: Lua writes JSON lines to a temp file, then TypeScript
post-processes the rendered HTML by finding a magic comment marker
(`<!-- htmldependencies:E3FAD763 -->`) and injecting `<link>`/`<script>`
tags there.

We don't need any of that. Our Lua runs in-process, and we control the
template. The approach:

1. **HTML deps (CSS/JS files)**: Extract from Lua → store as artifacts
   with paths → template renders `<link>` and `<script>` tags →
   post-pipeline writes files to disk (native) or VFS (WASM)
2. **Text includes**: Extract from Lua → push onto `PandocIncludes`
   on `StageContext` → template renders via `$for(header-includes)$`,
   `$for(include-before)$`, `$for(include-after)$` placeholders

### Pipeline architecture

Stages run sequentially sharing `&mut StageContext`:

```
ParseDocument → EngineExecution → MetadataMerge → CompileThemeCss →
  UserFilters(pre) → AstTransforms → UserFilters(post) →
  RenderHtmlBody → ApplyTemplate
```

- **EngineExecution** returns `ExecuteResult` with `includes:
  PandocIncludes` — currently **discarded** (pre-existing gap)
- **AstTransforms** runs shortcodes via `ShortcodeResolveTransform`
- **UserFilters** calls `apply_lua_filters` which (after Plan B)
  returns deps and includes via `FilterOutput`
- **ApplyTemplate** renders the HTML template with CSS/JS/includes

### Key files

| File | Role |
|---|---|
| `crates/quarto-core/src/stage/context.rs` | `StageContext` — shared pipeline context |
| `crates/quarto-core/src/stage/data.rs` | `PandocIncludes` struct (exists, partially wired) |
| `crates/quarto-core/src/artifact.rs` | `ArtifactStore` with `get_by_prefix()` |
| `crates/quarto-core/src/stage/stages/engine_execution.rs` | Discards `result.includes` (bug) |
| `crates/quarto-core/src/stage/stages/user_filters.rs` | Calls `apply_lua_filters`, pushes diagnostics |
| `crates/quarto-core/src/transforms/shortcode_resolve.rs` | Shortcode dispatch — calls pampa Lua engine |
| `crates/quarto-core/src/stage/stages/apply_template.rs` | Template stage — currently only handles `css:default` |
| `crates/quarto-core/src/template.rs` | HTML templates + `render_with_compiled_template()` |
| `crates/quarto-core/src/render_to_file.rs` | Native file output — currently only writes `css:default` |
| `crates/wasm-quarto-hub-client/src/lib.rs` | WASM — loops all artifacts with paths to VFS (already works) |

### Current template state

Both HTML templates (minimal and full) have `$if(header-includes)$` in
`<head>` — this is a **conditional, not a loop**. Quarto 1 (TS) uses
`$for(header-includes)$` loops for all three include variables, with
array values. We need to change to `$for()$` loops to match.

Neither template has `$for(include-before)$` or `$for(include-after)$`.
Neither has a `$for(scripts)$` loop. CSS is handled via `$for(css)$`
producing `<link>` tags.

`render_with_compiled_template()` accepts `body`, `meta`, `css_paths`
and builds the template context. `header-includes` only gets set if
it's in document metadata — no code path injects it programmatically.

### Current artifact flow for CSS

1. `CompileThemeCssStage` stores `"css:default"` artifact with path
2. `ApplyTemplateStage` checks if `"css:default"` exists, falls back to
   built-in default CSS; passes CSS path to template via `css_paths`
3. `render_to_file` reads `"css:default"` artifact, writes to
   `{stem}_files/styles.css`
4. WASM loops all artifacts with paths → writes to VFS

---

## Work Items

### Phase 1: `FilterOutput` struct + return type widening (pampa)

- [x] **1.1** In pampa, introduce a `FilterOutput` struct to replace
  the current 3-tuple return type:

  ```rust
  pub struct FilterOutput {
      pub pandoc: Pandoc,
      pub context: ASTContext,
      pub diagnostics: Vec<DiagnosticMessage>,
      pub html_dependencies: Vec<HtmlDependency>,
      pub text_includes: Vec<TextInclude>,
  }
  ```

  Change `apply_lua_filter` and `apply_lua_filters` to return
  `FilterResult<FilterOutput>`. Remove the `// TODO(plan-b)` discard
  from Plan A — actually return the extracted deps and includes.

- [x] **1.2** Update `unified_filter.rs::apply_filter()` and
  `apply_filters()` to pass through the `FilterOutput` struct.
  `apply_filters()` accumulates all three vectors (diagnostics,
  html_dependencies, text_includes) across multiple filter passes.

- [x] **1.3** Update test call sites in `filter_tests.rs`. Most tests
  call `apply_lua_filter(...).unwrap()` without destructuring — these
  need no change since the `.unwrap()` is transparent to the return
  type. Only ~1 test destructures the tuple explicitly:
  ```rust
  let (_, _, diagnostics) = apply_lua_filters(...);
  ```
  Change that to `output.diagnostics`. Minimal mechanical work.

- [x] **1.4** Export `FilterOutput` from `crates/pampa/src/lua/mod.rs`.
  (Note: `HtmlDependency`, `TextInclude`, `IncludeLocation`, and the
  extraction functions are already exported by Plan A.)

### Phase 2: `PandocIncludes` on `StageContext` + fix knitr gap

- [x] **2.1** Add `includes: PandocIncludes` field to `StageContext`
  (in `context.rs`). Initialize with `PandocIncludes::default()`.
  `PandocIncludes` already exists in `stage/data.rs` with:
  ```rust
  pub struct PandocIncludes {
      pub header_includes: Vec<String>,
      pub include_before: Vec<String>,
      pub include_after: Vec<String>,
  }
  ```

- [x] **2.2** In `engine_execution.rs`, after getting `result` from
  `engine.execute()`, save `result.includes` onto `ctx.includes`:
  ```rust
  ctx.includes.header_includes.extend(result.includes.header_includes);
  ctx.includes.include_before.extend(result.includes.include_before);
  ctx.includes.include_after.extend(result.includes.include_after);
  ```
  This fixes the pre-existing gap where knitr-generated includes are
  silently discarded. Note: the markdown engine early-returns at line
  189 without calling `execute()`, so this only affects knitr/jupyter.

- [x] **2.3** Unit test: verify that `EngineExecutionStage` preserves
  includes onto `StageContext`. **Requires** a mock engine that returns
  non-empty includes — the markdown engine never calls `execute()`.

### Phase 3: Wire shortcode extraction into pipeline

**Architecture note**: `ShortcodeResolveTransform::transform()` receives
`&mut RenderContext`, not `&mut StageContext`. `RenderContext` already has
an `artifacts: ArtifactStore` field (bridged to/from `StageContext` via
`std::mem::take` in `AstTransformsStage`). However, `RenderContext` does
**not** have an `includes` field — we need to add one.

- [x] **3.0** Add `includes: PandocIncludes` field to `RenderContext`
  (in `render.rs`). Initialize with `PandocIncludes::default()`.
  Update `AstTransformsStage` in `ast_transforms.rs` to bridge
  `includes` to/from `StageContext` the same way `artifacts` and
  `diagnostics` are already bridged:
  ```rust
  render_ctx.includes = std::mem::take(&mut ctx.includes);
  // ... execute transforms ...
  ctx.includes = render_ctx.includes;
  ```

- [x] **3.1** In `shortcode_resolve.rs`, after all shortcodes have been
  resolved (the Lua engine is about to be dropped), call the extraction
  methods added by Plan A:
  ```rust
  let diagnostics = lua_engine.extract_diagnostics()?;
  let html_deps = lua_engine.extract_html_dependencies()?;
  let text_includes = lua_engine.extract_text_includes()?;
  ```
  Push diagnostics onto `ctx.diagnostics`. This fixes the pre-existing
  gap where shortcode `quarto.warn()` calls are silently lost.

- [x] **3.2** For each `HtmlDependency`, read file contents via the
  runtime and store as artifacts. Artifact paths follow Quarto 1's
  `libs/` directory convention:
  - For each stylesheet: `ctx.artifacts.store("css:{dep.name}:{filename}",
    Artifact::from_bytes(content, "text/css").with_path(...))`
  - For each script: `ctx.artifacts.store("js:{dep.name}:{filename}",
    Artifact::from_bytes(content, "text/javascript").with_path(...))`
  - Artifact paths: `libs/{dep.name}/{filename}`
    (e.g. `libs/kbd/kbd.css`), relative to `{stem}_files/`
  - With version: `libs/{dep.name}-{dep.version}/{filename}`
  - This matches TS Quarto's `{stem}_files/libs/{name}/` layout

- [x] **3.3** For each `TextInclude`, push onto `ctx.includes`:
  ```rust
  match include.location {
      IncludeLocation::InHeader => ctx.includes.header_includes.push(include.content),
      IncludeLocation::BeforeBody => ctx.includes.include_before.push(include.content),
      IncludeLocation::AfterBody => ctx.includes.include_after.push(include.content),
  }
  ```

- [x] **3.4** Unit/integration test: a shortcode that calls
  `quarto.doc.add_html_dependency()` and `quarto.doc.include_text()`.
  After the shortcode resolve transform runs, verify artifacts are stored
  and includes are on the context.

### Phase 4: Wire filter extraction into pipeline

- [x] **4.1** In `user_filters.rs`, update the destructuring to use
  the new `FilterOutput` struct from Phase 1:

  Currently (line 137-148):
  ```rust
  let (new_ast, new_context, diagnostics) = pampa::unified_filter::apply_filters(...)?;
  ctx.diagnostics.extend(diagnostics);
  ```

  After:
  ```rust
  let output = pampa::unified_filter::apply_filters(...)?;
  ctx.diagnostics.extend(output.diagnostics);
  // Store html_deps as artifacts (same logic as Phase 3.2)
  // Push text_includes onto ctx.includes (same logic as Phase 3.3)
  ```

  Extract the artifact-storage and include-push logic into a helper
  function shared with Phase 3.

- [x] **4.2** Unit test: a filter that calls `add_html_dependency()` and
  `include_text()`, run through `UserFiltersStage`, verify artifacts and
  includes reach `StageContext`.

### Phase 5: Template updates

- [x] **5.1** Update both HTML templates in `template.rs`:

  **Both templates** — change `$if(header-includes)$` to a `$for()$`
  loop to match Quarto 1 (TS Quarto uses `$for()$` for all three
  include variables, with array values):
  ```html
  $for(header-includes)$
  $header-includes$
  $endfor$
  ```

  **Both templates** — add a scripts loop in `<head>`, after the CSS
  loop:
  ```html
  $for(scripts)$
  <script src="$scripts$"></script>
  $endfor$
  ```

  **Minimal template** — add `include-before` and `include-after`
  around `$body$`:
  ```html
  $for(include-before)$
  $include-before$
  $endfor$
  $body$
  $for(include-after)$
  $include-after$
  $endfor$
  ```

  **Full template** — same placement logic: `include-before` after
  `<body>` opens (before `<div id="quarto-content">`), `include-after`
  after `</main>` (before `</body>`).

  (TS Quarto puts dependency scripts in `<head>` by default.)

- [x] **5.2** Update `render_with_compiled_template()` in `template.rs`
  to accept includes and script paths. Add parameters (or a struct):
  ```rust
  pub fn render_with_compiled_template(
      template: &Template,
      body: &str,
      meta: &ConfigValue,
      css_paths: &[String],
      script_paths: &[String],        // NEW
      includes: &PandocIncludes,       // NEW
  ) -> Result<String>
  ```

  In the function body, set all three include variables as **lists**
  (matching Quarto 1 behavior — all three are arrays, not strings):
  - Set `header-includes` as a list of raw HTML strings
  - Set `include-before` as a list of raw HTML strings
  - Set `include-after` as a list of raw HTML strings
  - Set `scripts` as a list of path strings
  - If metadata also has `header-includes`, merge (metadata entries
    first, then programmatic entries)

- [x] **5.3** Update all callers of `render_with_compiled_template`:
  - `ApplyTemplateStage::run()` — read includes from `ctx.includes`,
    collect script paths from `js:*` artifacts, pass to renderer
  - `render_with_resources()` — pass empty defaults
  - `render_with_format()` — pass empty defaults

- [x] **5.4** In `ApplyTemplateStage::run()`, collect dependency
  artifact paths for the template:
  ```rust
  // Collect all CSS paths (default + extension deps)
  let css_paths = /* existing logic + collect from "css:*" artifacts */;

  // Collect all JS paths from "js:*" artifacts
  let script_paths: Vec<String> = ctx.artifacts
      .get_by_prefix("js:")
      .iter()
      .filter_map(|(_, a)| a.path.as_ref())
      .map(|p| p.to_string_lossy().to_string())
      .collect();
  ```

- [x] **5.5** Unit tests:
  - Template renders `<link>` tags for multiple CSS paths
  - Template renders `<script src="...">` tags for script paths
  - Template renders header-includes content in `<head>`
  - Template renders include-before content before body
  - Template renders include-after content after body
  - Empty includes/scripts produce no extra tags

### Phase 6: `render_to_file` updates

- [x] **6.1** In `render_to_file.rs`, after the existing CSS write
  (which writes `"css:default"` to `{stem}_files/styles.css`), add a
  loop that writes all other artifact files:
  ```rust
  for (key, artifact) in ctx.artifacts.iter() {
      if let Some(path) = &artifact.path {
          // Skip "css:default" (already written above)
          if key == "css:default" { continue; }
          // Write CSS and JS artifacts to the resource dir
          if key.starts_with("css:") || key.starts_with("js:") {
              let output_path = resource_dir.join(path);
              // Create parent directory if needed
              runtime.dir_create(output_path.parent().unwrap(), true)?;
              runtime.file_write(&output_path, &artifact.content)?;
          }
      }
  }
  ```

  WASM already handles this — the existing artifact→VFS loop at the
  end of `render_qmd_content` writes all artifacts with paths. No
  WASM changes needed.

- [x] **6.2** Unit test: verify that CSS/JS artifacts with paths get
  written to the output directory.

### Phase 7: Integration tests

- [x] **7.1** Smoke test: covered by built-in `kbd` extension
  (batch 2 plan). The kbd smoke test verifies `add_html_dependency`
  end-to-end: `<link>` for CSS, `<script>` for JS, files in
  `{stem}_files/libs/kbd/`.

- [x] **7.2** Smoke test: covered by built-in `video` extension
  (video plan). The `builtin-video-local` smoke test exercises
  `include_text("after-body", ...)` for VideoJS initialization scripts.

- [x] **7.3** Smoke test: covered by built-in `kbd` extension
  (batch 2 plan). The kbd shortcode calls `quarto.doc.is_format()`
  and `quarto.doc.isFormat()` to produce format-dependent output.

- [x] **7.4** `cargo nextest run --workspace` — no regressions

- [x] **7.5** WASM build + hub-client tests pass (52 tests).
  `cargo xtask verify` fails at tree-sitter step (pre-existing:
  `tree-sitter` CLI not installed), but all relevant steps pass.

---

## Design Notes

### `FilterOutput` struct replaces growing tuples

Plan A kept the return type of `apply_lua_filter` unchanged (3-tuple)
and discarded extraction results. Plan B introduces `FilterOutput` to
carry all results:

```rust
pub struct FilterOutput {
    pub pandoc: Pandoc,
    pub context: ASTContext,
    pub diagnostics: Vec<DiagnosticMessage>,
    pub html_dependencies: Vec<HtmlDependency>,
    pub text_includes: Vec<TextInclude>,
}
```

Benefits:
- Adding fields later is non-breaking (no tuple position changes)
- Test sites use `output.pandoc` instead of positional destructuring
- Self-documenting field names

Most filter test sites call `.unwrap()` without destructuring and need
no change. Only ~1 site explicitly destructures the tuple.

### Why artifacts for CSS/JS files, `PandocIncludes` for text

CSS/JS dependencies are **files** that need to be:
1. Referenced by path in HTML tags (`<link href="...">`,
   `<script src="...">`)
2. Written to the output directory (native) or VFS (WASM)

The artifact store handles exactly this — it stores content with an
associated path, and the post-pipeline output loop writes them.

Text includes are **inline raw HTML** that gets injected directly into
the document. They don't have file paths. `PandocIncludes` already
models this as `Vec<String>` for three locations, matching Pandoc's
template variables. Using `PandocIncludes` avoids inventing a new
mechanism and completes wiring that was already started (the struct
exists, the template has `$for(header-includes)$`, they just weren't
connected).

### `PandocIncludes` on both `StageContext` and `RenderContext`

`StageContext` gets `includes: PandocIncludes` for pipeline-level
accumulation (engine execution, user filters). `RenderContext` also gets
`includes: PandocIncludes` so that AST transforms (shortcodes) can push
text includes. The `AstTransformsStage` bridges both via `std::mem::take`,
the same pattern already used for `artifacts` and `diagnostics`.

HTML deps go straight from Lua extraction to artifact storage. Text
includes go straight from Lua extraction to `ctx.includes`. No other
new struct fields needed.

### Template variable names match Pandoc

We use `header-includes`, `include-before`, `include-after` — the same
names Pandoc's default HTML5 template uses. This ensures compatibility
with extensions that set these via document metadata (which flows
through `add_metadata_to_context_except` in `template.rs`).

### Dependency file paths match Quarto 1's `libs/` convention

TS Quarto writes HTML dependency files to `{stem}_files/libs/{name}/`
(or `{name}-{version}/` with a version). External (contrib) deps go to
`{stem}_files/libs/quarto-contrib/{name}/`. We follow the same layout:

- Artifact paths are `libs/{dep.name}/{filename}` (relative to
  `{stem}_files/`)
- With version: `libs/{dep.name}-{dep.version}/{filename}`
- HTML tags use the full relative path:
  `{stem}_files/libs/{dep.name}/{filename}`

### WASM needs no changes for artifact file output

The existing loop in `wasm-quarto-hub-client/src/lib.rs` already writes
all artifacts with paths to VFS:
```rust
for (_key, artifact) in ctx.artifacts.iter() {
    if let Some(path) = &artifact.path {
        runtime.add_file(path, artifact.content.clone());
    }
}
```
New `css:*` and `js:*` artifacts will be picked up automatically.

---

## Files Touched

| File | Change |
|---|---|
| `crates/pampa/src/lua/filter.rs` | Return `FilterOutput` instead of 3-tuple; stop discarding deps/includes |
| `crates/pampa/src/lua/mod.rs` | Export `FilterOutput` |
| `crates/pampa/src/lua/filter_tests.rs` | Update ~1 test site that destructures the tuple |
| `crates/pampa/src/unified_filter.rs` | Pass through `FilterOutput`; accumulate deps/includes |
| `crates/quarto-core/src/render.rs` | Add `includes: PandocIncludes` field to `RenderContext` |
| `crates/quarto-core/src/stage/context.rs` | Add `includes: PandocIncludes` field to `StageContext` |
| `crates/quarto-core/src/stage/stages/ast_transforms.rs` | Bridge `includes` between `StageContext` and `RenderContext` |
| `crates/quarto-core/src/stage/stages/engine_execution.rs` | Save `result.includes` to context |
| `crates/quarto-core/src/transforms/shortcode_resolve.rs` | Extract deps/includes/diagnostics from Lua engine |
| `crates/quarto-core/src/stage/stages/user_filters.rs` | Consume `FilterOutput`, store deps/includes |
| `crates/quarto-core/src/template.rs` | Add template placeholders, update `render_with_compiled_template` |
| `crates/quarto-core/src/stage/stages/apply_template.rs` | Collect artifact paths, pass includes to template |
| `crates/quarto-core/src/render_to_file.rs` | Write all `css:*`/`js:*` artifacts to output dir |
| Smoke test fixtures | New: integration tests for deps, includes, is_format |
| `hub-client/src/utils/iframePostProcessor.ts` | Phase 8: comment out script inlining |
| `hub-client/src/components/render/DoubleBufferedIframe.tsx` | Phase 8: revert allow-scripts (separate revert commit) |
| `hub-client/src/components/render/MorphIframe.tsx` | Phase 8: revert allow-scripts (separate revert commit) |

---

## Phase 8: Disable WASM script execution (pullback)

**Context**: The JS injection pipeline works end-to-end for native renders:
Lua `add_html_dependency` → artifact storage → template `<script>` tags →
files written to `{stem}_files/libs/`. This is fine and stays enabled.

However, for the WASM/hub-client preview iframe, executing extension JS
requires two things that were added prematurely:
1. Script inlining in `iframePostProcessor.ts` (reads JS from VFS,
   creates inline `<script>` elements, dispatches synthetic DOMContentLoaded)
2. `allow-scripts` in the iframe sandbox attribute

Until we determine a safe way to run extension scripts in the sandboxed
iframe, both are disabled. Extensions with JS dependencies (kbd, video)
will render their HTML structure but JS won't execute in the hub-client
preview. They continue to work correctly in native renders.

### Changes

- [x] **8.1** Revert commit `0c426798` ("Allow script execution in
  preview iframe sandbox") — restores `sandbox="allow-same-origin
  allow-popups"` without `allow-scripts` on both `DoubleBufferedIframe`
  and `MorphIframe`. Done as a separate `git revert` commit for
  traceability.

- [x] **8.2** Comment out (not remove) the script inlining block in
  `iframePostProcessor.ts`. The comment explains why it's disabled and
  what to re-enable (the block itself + allow-scripts on the iframes).
  CSS inlining via data URIs is unaffected — CSS does not execute code.

### What stays enabled

- Template `$for(scripts)$` loop — emits `<script src="...">` in HTML
- `js:*` artifact collection in `ApplyTemplateStage`
- `render_to_file` writing JS artifacts to `{stem}_files/libs/`
- WASM artifact→VFS loop (writes JS files, they just aren't inlined)
- `add_html_dependency` Lua API (stores artifacts normally)
- kbd, video, and all other built-in extensions

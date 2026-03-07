# Plan: Switch Hub-Client to render_qmd

## Context for New Agents

This plan changes how the hub-client's live preview renders documents. Read this section
before doing anything else.

### Architecture

The hub-client is a React/TypeScript app in `hub-client/`. It uses a WASM build of the
Rust rendering engine (`wasm-quarto-hub-client`) for live preview. The WASM module has a
**Virtual File System (VFS)** that mirrors the project files. The Automerge sync layer
(`src/services/automergeSync.ts`) keeps the VFS in sync with the server — every file
add/change/remove triggers a corresponding `vfs_add_file()` / `vfs_remove_file()` call.

### Current rendering flow (what we're changing)

```
Editor content string
  → Preview.tsx calls renderToHtml(content, { sourceLocation, documentPath })
  → wasmRenderer.ts calls render_qmd_content(content) or render_qmd_content_with_options(content, opts)
  → WASM creates a temporary /input.qmd, renders it with NO project context
  → compileAndInjectThemeCss() runs separately for CSS
  → HTML returned to Preview component → displayed in iframe
```

The problem: `render_qmd_content` doesn't see `_quarto.yml`, `_metadata.yml`, or any
project structure. It renders the document in isolation.

### Target rendering flow (what we're building)

```
Automerge keeps VFS in sync (already working)
  → Preview.tsx calls renderToHtml({ documentPath })
  → wasmRenderer.ts calls render_qmd(documentPath)
  → WASM reads from VFS, discovers _quarto.yml, merges all metadata layers
  → HTML returned with full project context (ToC, themes, directory metadata, etc.)
```

The content is NOT passed as a string — it's already in VFS via Automerge sync.
Source location for scroll sync is set via runtime metadata (see below), not a per-render option.

### Runtime metadata (already implemented)

Commit `e7f1e61d` added a runtime metadata layer to the WASM module. This lets the
hub-client inject metadata at the highest precedence (above project, directory, document):

```typescript
// WASM entry points (already exported and typed):
vfs_set_runtime_metadata(yaml: string): string;  // returns JSON {success, error?}
vfs_get_runtime_metadata(): string;               // returns JSON {success, content?}
```

For scroll sync, the hub-client will call:
```typescript
vfs_set_runtime_metadata('format:\n  html:\n    source-location: full\n')
```

This causes `data-loc` attributes in rendered HTML, which `useScrollSync` uses.
Already tested in `src/services/runtimeMetadata.wasm.test.ts` (12 tests, all passing).

### Key files

| File | What it does |
|------|-------------|
| `hub-client/src/services/wasmRenderer.ts` | WASM wrapper service — main changes go here |
| `hub-client/src/components/Preview.tsx` | Preview component — calls renderToHtml |
| `hub-client/src/components/tabs/AboutTab.tsx` | Renders changelog markdown — keep using render_qmd_content |
| `hub-client/src/services/automergeSync.ts` | Keeps VFS in sync — no changes needed |
| `hub-client/src/test-utils/mockWasm.ts` | Mock WASM for unit tests |
| `hub-client/src/types/wasm-quarto-hub-client.d.ts` | TypeScript declarations for WASM functions |

### VFS path convention

All VFS paths use `/project/` prefix. Automerge paths are relative (e.g. `"index.qmd"`).
The VFS `normalize_path()` resolves relative paths to `/project/index.qmd`. Both
`vfs_add_file` and `render_qmd` go through this normalization, so relative paths from
Automerge work consistently — no `/project/` prefix needed from TypeScript.

---

## Work Items

### Phase 1: TypeScript API Changes

- [x] Add `vfs_set_runtime_metadata` / `vfs_get_runtime_metadata` to TypeScript type declarations
  - Done in `e7f1e61d`

- [x] Add wrapper functions in `wasmRenderer.ts`
  - `setRuntimeMetadata(yaml: string): VfsResponse` — wraps `vfs_set_runtime_metadata`
  - `getRuntimeMetadata(): VfsResponse` — wraps `vfs_get_runtime_metadata`
  - `setScrollSyncEnabled(enabled: boolean): void` — convenience that updates a
    TypeScript-side settings object and flushes the full blob to WASM:
    - Maintains `runtimeSettings: Record<string, unknown>` internally
    - `true` → sets `runtimeSettings.format = { html: { 'source-location': 'full' } }`
    - `false` → deletes `runtimeSettings.format`
    - Serializes the whole object to YAML and calls `setRuntimeMetadata(yaml)`
    - This way multiple runtime settings can coexist without clobbering each other

- [x] Update `RenderToHtmlOptions` interface (currently at line ~552 of `wasmRenderer.ts`)
  - Remove `sourceLocation?: boolean` (now handled via runtime metadata)
  - Make `documentPath` required (was optional, defaulted to `"input.qmd"`)
  - Current interface:
    ```typescript
    interface RenderToHtmlOptions {
      sourceLocation?: boolean;
      documentPath?: string;
    }
    ```

- [x] Update `renderToHtml` implementation (currently at line ~647 of `wasmRenderer.ts`)
  - Change signature: no longer takes `qmdContent` as first arg
  - Call `renderQmd(documentPath)` instead of `renderQmdContent(content)`
  - Remove the `renderQmdContentWithOptions` code path
  - **Theme CSS**: `render_qmd` in Rust already writes CSS artifacts to VFS at
    `/.quarto/project-artifacts/styles.css` (lib.rs lines 724-728). However, the
    JS-side `compileAndInjectThemeCss` uses dart-sass for theme compilation, which
    is separate from the Rust pipeline. After the switch, read content from VFS via
    `vfsReadFile(documentPath)` to feed `compileAndInjectThemeCss`. The function
    already accepts `documentPath` for relative theme resolution.

### Phase 2: Component Updates

- [x] Update `Preview.tsx`
  - In `doRender()` (line ~208): stop passing `qmdContent` to `renderToHtml`
  - Pass `documentPath` (required) — available as `currentFile?.path`
  - Remove `sourceLocation: options.scrollSyncEnabled` from render options
  - Instead, call `setScrollSyncEnabled()` when `scrollSyncEnabled` prop changes
    (use a useEffect or similar — set it once, not per-render)
  - **Race condition note:** `vfsAddFile` is synchronous and happens before
    `onFileContent` callback fires, which triggers the render. So VFS is always
    up-to-date when `render_qmd` reads it.

- [x] Update `AboutTab.tsx` (line ~84)
  - Currently calls `renderToHtml(doc.markdown)` with no options
  - This must keep working — AboutTab renders static markdown (changelog, more-info)
    that has no project context
  - **Decision:** Add a `renderContentToHtml(content: string)` convenience function in
    `wasmRenderer.ts` that wraps `renderQmdContent` directly (no VFS, no project context).
    AboutTab calls this instead of `renderToHtml`. This cleanly separates the two paths:
    `renderToHtml(opts)` for VFS-based project rendering, `renderContentToHtml(content)`
    for standalone content rendering.

- [x] Verify `PreviewRouter.tsx` needs no changes
  - Uses `parseQmdToAst(props.content)` for format detection (slides vs. document)
  - This is parsing, not rendering — no project context needed, no changes needed

### Phase 3: Cleanup

- [x] Deprecate `render_qmd_content_with_options` code path
  - Remove `renderQmdContentWithOptions` from `wasmRenderer.ts`
  - Remove `WasmRenderOptions` interface (line ~290, currently `{ sourceLocation?: boolean }`)
  - Keep the Rust WASM function for now — remove in a follow-up

- [x] Update mock WASM in `mockWasm.ts`
  - Add `vfs_set_runtime_metadata`, `vfs_get_runtime_metadata` to mock
  - Update `renderToHtml` mock to match new signature (no content arg)
  - Keep `renderQmdContent` mock for AboutTab path

### Phase 4: Tests

- [x] **WASM integration: runtime metadata → data-loc**
  - Done in `e7f1e61d` — `runtimeMetadata.wasm.test.ts` (12 tests)

- [x] **WASM integration: render_qmd with project context**
  - Done in `e7f1e61d` — tests verify runtime metadata overrides project and document

- [x] **WASM integration: render_qmd picks up _metadata.yml**
  - Populate VFS with `_quarto.yml`, `chapters/_metadata.yml` (`author: "Dir Author"`),
    and `chapters/doc.qmd`
  - Call `render_qmd("/project/chapters/doc.qmd")`
  - Verify rendered HTML includes directory metadata author

- [x] **Unit test: renderToHtml calls render_qmd**
  - Covered by WASM integration tests: `render_qmd` with project context, directory
    metadata, and relative paths all pass. TypeScript type system enforces the new
    signature (`documentPath` required, no `content` arg).

- [x] **Unit test: setScrollSyncEnabled / toSimpleYaml**
  - `toSimpleYaml` unit tests in `wasmRenderer.test.ts` (6 tests): flat keys, nested
    objects, booleans/numbers, empty object, mixed keys, exact scroll-sync YAML output
  - WASM integration tests cover the full pipeline: `vfs_set_runtime_metadata` → `data-loc`

- [x] **Component test: Preview renders via render_qmd**
  - TypeScript type system enforces the new API. `renderToHtml` no longer accepts a
    content string argument. The 322 unit tests all pass with the new signature.

- [ ] **E2E: theme CSS compilation**
  - Verify theme CSS still works after the switch
  - `compileAndInjectThemeCss` now reads content from VFS via `vfsReadFile(documentPath)`
  - Requires manual browser testing (dart-sass runs in browser context)

## Resolved Questions

1. ~~**Theme CSS compilation**~~: **Resolved.** The Rust `render_qmd` writes CSS artifacts
   to VFS (lib.rs lines 724-728), but this is the Rust-side pipeline — the JS-side
   dart-sass `compileAndInjectThemeCss` is separate and still needed for theme compilation.
   After the switch, read content from VFS via `vfsReadFile(documentPath)` to feed it.
   `compileDocumentCss` already accepts a `documentPath` parameter for relative theme
   resolution, so only the content source changes.

2. ~~**VFS path normalization**~~: **Resolved.** VFS `normalize_path()` prepends `/project/`
   to relative paths. Both `vfs_add_file` and `render_qmd` go through normalization, so
   relative Automerge paths (e.g. `"index.qmd"`) work without adding a `/project/` prefix
   from TypeScript. Verified in `wasm.rs` lines 366-374.

3. ~~**`setScrollSyncEnabled(false)` clearing all metadata**~~: **Resolved.** The
   TypeScript layer should own a settings object and serialize the whole thing to YAML
   on each change. The WASM API stays all-or-nothing (one opaque blob), and the TS
   wrapper composes multiple settings. The runtime metadata goes through
   `resolve_format_config` in `AstTransformsStage`, so nested format-specific keys
   (e.g. `format.html.source-location`) are flattened correctly during the deep merge.

## Files Modified

- `hub-client/src/services/wasmRenderer.ts` — main changes (new wrappers, renderToHtml rewrite)
- `hub-client/src/components/Preview.tsx` — render call changes
- `hub-client/src/components/tabs/AboutTab.tsx` — keep standalone render path
- `hub-client/src/test-utils/mockWasm.ts` — mock updates
- `hub-client/src/services/runtimeMetadata.wasm.test.ts` — may extend with _metadata.yml test

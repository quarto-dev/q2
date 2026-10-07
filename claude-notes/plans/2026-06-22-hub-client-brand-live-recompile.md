# hub-client: live `_brand.yml` change doesn't recompile preview CSS (AST/slides path)

**Strand:** bd-4jjckvwt
**Date:** 2026-06-22
**Status:** diagnosis — awaiting go-ahead to implement

## Symptom

In a live hub-client session viewing a `format: revealjs` deck (e.g.
`slides.qmd`) with a `_brand.yml` sibling, when `_brand.yml` changes — typically
arriving via Automerge sync from another collaborator — the preview's **theme
CSS is not recompiled**. The brand styling only updates after a manual page
reload. The HTML preview (`format: html`) updates correctly under the same
conditions.

### Reproduction (why two windows)

The bug shows only when the active view stays on `slides.qmd` *while*
`_brand.yml` changes underneath it:

- Window A: open the project, view `slides.qmd`.
- Window B (or a collaborator): edit `_brand.yml`.
- The change syncs into A's Automerge doc, but A's slide preview keeps the old
  brand CSS until reload.

A single window masks it: editing `_brand.yml` means you're *viewing*
`_brand.yml`, and switching back to `slides.qmd` changes `currentFile.path`,
which re-renders anyway. So it's specifically the collaborative / background-edit
case.

## Root cause — one asymmetric React effect dependency

hub-client has two preview surfaces with **inconsistent re-render triggers**:

### HTML path — correct (`Preview.tsx`)

`hub-client/src/components/render/Preview.tsx:343-354` — the re-render effect
**depends on `fileContents`**:

```ts
useEffect(() => {
  const filePath = currentFile?.path;
  if (!filePath) return;
  updatePreview(content, filePath);
}, [content, fileContents, updatePreview, currentFile?.path]); // fileContents ✓
```

`fileContents` is a `Map` that gets a **fresh identity on every Automerge edit**
(`App.tsx:431-438` `setFileContents(prev => new Map(prev)…)`), so any sibling
edit — including `_brand.yml` — re-fires the effect and re-renders the active
page. This is the documented "Phase 9 Decision 6" behavior, and the 20 ms
debounce inside `updatePreview` absorbs bursts.

### AST / slides path — broken (`ReactPreview.tsx`)

`hub-client/src/components/render/ReactPreview.tsx:647-657` — the equivalent
effect **omits `fileContents`**:

```ts
useEffect(() => {
  updatePreview(content, currentFile?.path);
}, [
  content,
  updatePreview,
  scrollSyncEnabled,
  currentFile?.path,
  onDiagnosticsChange,
  attributionPayload,
  // fileContents MISSING ✗
]);
```

`content` is only the **active document's** text (`slides.qmd`). A `_brand.yml`
edit doesn't change `content`, and `fileContents` isn't a dependency, so the
effect never re-fires → no re-render → no theme recompile. This path serves
**all AST-routed formats** — `revealjs`, `q2-preview`, `q2-slides`, `q2-debug` —
so the bug is not revealjs-specific.

## Everything downstream already works

The fix is *only* the missing trigger; the rest of the chain is already correct:

1. **VFS stays fresh.** `ts-packages/preview-runtime/src/automergeSync.ts:98-102`
   `onFileChanged` calls `vfsAddFile(path, text)` for **every** changed file,
   `_brand.yml` included. So the WASM VFS that `renderPageForPreview(path)` reads
   from already has the new brand bytes. (The agent's "sibling VFS sync" concern
   is moot — the live sync client does it; the only per-active-file `vfsAddFile`
   in `Editor.tsx:450-473` is the *replay* path, a separate mode.)
2. **A re-render recompiles the theme.** `CompileThemeCssStage`\'s cache key
   includes the resolved brand's YAML
   (`crates/quarto-core/src/stage/stages/compile_theme_css.rs:202-214`), so a
   brand change yields a **different `css:theme:<fp>` key** → cache miss →
   recompile → new fingerprint flows to the iframe via the existing
   `themeFingerprint` → `UPDATE_THEME` transport. No stale-fingerprint
   short-circuit.
3. **`renderPageForPreview` reads brand from the fresh VFS** and resolves the
   reveal/HTML theme with the new `_brand.yml`.

So: trigger the re-render, and correct brand CSS follows automatically.

## Fix

### Primary (recommended): parity with the HTML path

Add `fileContents` to `ReactPreview`\'s re-render effect dependency array
(`ReactPreview.tsx:650-657`), mirroring `Preview.tsx`:

```ts
}, [
  content,
  fileContents,   // ← add: re-render on any sibling edit (incl. _brand.yml)
  updatePreview,
  scrollSyncEnabled,
  currentFile?.path,
  onDiagnosticsChange,
  attributionPayload,
]);
```

- **Pro:** one line; restores symmetry with the HTML path; uses the established
  coarse "re-render on any sibling content change" pattern; fixes `_quarto.yml`,
  `_metadata.yml`, and `_brand.yml` (and any future config sibling) at once, for
  every AST-path format.
- **Con:** re-renders the deck on *any* sibling edit, not just relevant ones.
  This matches `Preview.tsx` exactly and the 20 ms debounce absorbs bursts, so
  it's acceptable. For decks this is a full WASM pipeline pass per burst — see
  the optional optimization below if that proves costly.

### Optional follow-up (not required): fine-grained dependency tracking

The user framed it as "know that `slides.qmd`\'s style depends on `_brand.yml`
and recompile when *that* changes." That's a real optimization — re-render only
when a file the active doc actually depends on (config/brand siblings, included
files) changes, rather than on every sibling keystroke. But it adds a
dependency-manifest mechanism the codebase doesn't have yet, and the HTML path
deliberately doesn't do it. **Recommend shipping the coarse fix first** (parity
+ correctness), and only pursuing fine-grained tracking if profiling shows the
extra deck re-renders matter. File as a separate optimization strand if desired.

## Verification plan (TDD) — DONE

- [x] Integration test `ReactPreview.rerender.integration.test.tsx`: rendering
  `ReactPreview` (format: revealjs) and swapping `fileContents` to a new Map
  identity (a `_brand.yml` edit) must re-invoke `renderPageForPreview`. Red
  before the one-line fix, green after. (commit `f4e2c25e`)
- [x] **Live two-window check** on the running hub-client (the user's own two
  windows on project `8e957fe2`): edited `_brand.yml`\'s `palette.blue`
  `#447099`→`#00cc00` in window B; window A's deck headings recompiled
  `rgb(68,112,153)`→`rgb(0,204,0)` with **no reload**, theme blob URL
  refreshed (`69cec62d…`→`96bc445d…`); reverted → headings back to blue
  immediately. Project restored to original.
- [x] hub-client `tsc -b`, `npm run test` (614), `npm run test:integration`
  (75), `npm run build` (production) — all green. No regressions.
- [x] No Rust/WASM change — pure TS.

## References
- Sibling-edit re-render pattern: `Preview.tsx:343-354`, `PreviewRouter.tsx`
  "Phase 9 Decision 6" comment.
- Automerge → VFS sync: `ts-packages/preview-runtime/src/automergeSync.ts:88-109`.
- Brand in theme cache key: `crates/quarto-core/src/stage/stages/compile_theme_css.rs:202-214`.
- Brand discovery for preview (added by `650cbddc`):
  `claude-notes/plans/2026-06-16-revealjs-render-preview-theme-parity.md`.

# Import a project from a ZIP archive (hub-client)

**Beads:** bd-apv23
**Status:** Design — awaiting user go-ahead before implementation.
**Date:** 2026-06-01

## Overview

hub-client can already **export** a project's contents to a ZIP
(`ts-packages/quarto-sync-client/src/export-zip.ts`, surfaced as the
"Export to ZIP" button on `ProjectTab`). This plan adds the inverse:
**import a project from a ZIP**. A user uploads a `.zip` from the
project-selector landing page, and we create a brand-new hub-client
project whose files are the contents of that archive — reusing the
existing "create new project" Automerge/IndexedDB/project-set
machinery.

Goal: a user can take a ZIP exported by hub-client (or a plausible
GitHub-style download of a Quarto project) and turn it into a new,
fully synced hub-client project in one action.

## Existing code this builds on

| Concern | Location | Notes |
| --- | --- | --- |
| ZIP export (the inverse) | `ts-packages/quarto-sync-client/src/export-zip.ts` | Uses `fflate` `zipSync`; `Record<path, Uint8Array>`. |
| `fflate` dependency | `ts-packages/quarto-sync-client/package.json` | `unzipSync`, `strToU8`, `strFromU8` available. Already a dep. |
| Create-project UI | `hub-client/src/components/ProjectSelector.tsx` | `handleCreateProject` → `wasmCreateProject` → `onProjectCreated(files, title, type, syncServer)`. |
| Create-project handler | `hub-client/src/App.tsx` `handleProjectCreated` (≈L475) | Maps scaffold `ProjectFile[]` → `CreateProjectOptions.files`, calls `createNewProject`, writes IDB + project set, navigates. |
| Core project creation | `ts-packages/quarto-sync-client/src/client.ts` `createNewProject` (≈L808) | Iterates `options.files`; **binary content is base64**, decoded via `atob` (L877); text content is a plain string. |
| File-shape: scaffold | `ts-packages/preview-runtime/src/wasmRenderer.ts` `ProjectFile` (L915) | snake_case: `{ path, content_type: 'text'\|'binary', content, mime_type? }`. |
| File-shape: create input | `ts-packages/quarto-sync-client/src/types.ts` `CreateProjectOptions` (L213) | camelCase: `{ path, content, contentType, mimeType? }`. |
| Binary classification | `ts-packages/quarto-automerge-schema/src/index.ts` `isBinaryExtension` (L393), `isTextExtension`, `inferMimeType` (L409) | Extension-based heuristics already used by the asset-upload path. |

Key fact that makes this tractable: **`createNewProject` already
accepts a heterogeneous file list with base64 binary content.** Import
is therefore "parse ZIP → build a file list in that shape → run the
existing create path." No changes to the Automerge layer are required.

## Architecture / data flow

```
ProjectSelector  ── user clicks "Import from ZIP", picks file + title + sync server
      │
      ├─ read File → ArrayBuffer → Uint8Array
      ├─ parseProjectZip(zipBytes)          [NEW: quarto-sync-client/src/import-zip.ts]
      │     unzipSync → Record<path, Uint8Array>
      │     strip common top-level dir, drop junk/dirs, zip-slip guard
      │     classify each entry (isBinaryExtension): text→strFromU8, binary→base64
      │     → ProjectFile[]  (snake_case, same shape wasmCreateProject returns)
      │
      └─ onProjectCreated(files, title, 'imported', syncServer)   [REUSED, unchanged]
            │
            └─ App.handleProjectCreated → createNewProject(...) → IDB + project set + navigate
```

### Design decision: reuse `onProjectCreated` (recommended)

Have `parseProjectZip` return `ProjectFile[]` (the **snake_case**
scaffold shape) so the result flows straight into the existing
`onProjectCreated` callback with zero changes to `App.handleProjectCreated`.
This maximizes reuse — IDB write, project-set add, and navigation all
come for free.

- The pure parse logic lives in `quarto-sync-client` (mirrors
  `export-zip.ts`, same package, same `fflate` dep). It returns the
  neutral camelCase `CreateProjectOptions['files']` shape that already
  lives in that package's `types.ts`.
- A thin wrapper in `preview-runtime`
  (`importProjectFromZip(zipBytes): ProjectFile[]`) converts to the
  snake_case `ProjectFile` shape and is what `ProjectSelector` imports
  — symmetric with how `exportProjectAsZip` is surfaced through
  `preview-runtime/src/automergeSync.ts`.

*Alternative considered:* a separate `handleProjectImported` in `App.tsx`
that calls `createNewProject` directly with the camelCase shape. Rejected
— it duplicates the IDB/project-set/navigation block. If we ever need
import-specific post-processing, refactor the shared tail of
`handleProjectCreated` into a helper rather than forking it.

### UI placement

Add an **"Import from ZIP"** button in the project-actions area next to
"Create New Project" / "Connect to Project" (NOT the bottom
"Import/Export from JSON" row — that row backs up the *project list
metadata*, a different concern from project *contents*).

Clicking opens a small form (reusing the create-form styling):
- **Project title** — text input, pre-filled from the ZIP filename
  (minus `.zip`), editable.
- **Sync server URL** — pre-filled from the current `syncServer` state
  (same default as the create form).
- **ZIP file** — `<input type="file" accept=".zip,application/zip">`.
- Submit is disabled until a file is chosen and title/sync-server are
  non-empty.

On submit: read the file, `parseProjectZip`, then `onProjectCreated`.
Surface parse errors inline via the existing `formError` channel.

## Edge cases & decisions (for review)

1. **Top-level directory stripping.** GitHub "Download ZIP" wraps
   everything in `repo-main/`. If *every* entry shares one leading path
   segment, strip it so paths become project-relative (`index.qmd`, not
   `repo-main/index.qmd`). A hub-client-exported ZIP has no such wrapper,
   so this is a no-op for round-trips. **Recommend: strip single common
   prefix.**
2. **Junk entries.** Skip directory entries (path ends in `/`),
   `__MACOSX/…`, `.DS_Store`, and `.git/…`. **Recommend: skip.**
3. **Binary vs text classification.** Use `isBinaryExtension`. Risk:
   an unknown extension defaults to text and a true-binary file would be
   mangled by UTF-8 decoding. **Recommend:** extension first, plus a
   UTF-8-decodability fallback — if a non-binary-extension entry fails to
   decode cleanly as UTF-8 (or contains NUL bytes), treat it as binary
   with `application/octet-stream`. Flag for discussion: is the extra
   sniffing worth it, or is extension-only acceptable for v1?
4. **Zip-slip / path safety.** Reject or normalize entries with absolute
   paths or `..` traversal. **Recommend: reject the whole import with a
   clear error** rather than silently dropping.
5. **Empty / no-usable-files ZIP.** Error out with a friendly message
   ("No files found in the archive").
6. **Invalid / corrupt ZIP.** `unzipSync` throws; catch and surface as
   `formError`.
7. **Synchronous unzip on the main thread.** `unzipSync` blocks; large
   archives could jank the UI. **v1: accept it**, but log a note and
   open a follow-up (`discovered-from`) to move to async `unzip`/worker
   if we hit large real-world archives.
8. **Title default.** ZIP filename sans extension; user-editable.
9. **No project-type / validation gate.** Unlike create, import does not
   pick a Quarto project type or require `_quarto.yml`. We import
   whatever is there. (Optional future: warn if no `.qmd`/`_quarto.yml`
   present.)
10. **Round-trip fidelity.** Importing a ZIP produced by "Export to ZIP"
    must reproduce the same file set + contents. This is the headline
    regression test.

## Work items (TDD — tests first)

### Phase 1 — Parse layer (pure, unit-tested) ✅ DONE
- [x] Write `ts-packages/quarto-sync-client/src/import-zip.test.ts`
      (mirror `export-zip.test.ts`) covering: basic text+binary,
      Unicode round-trip, nested paths, top-level-dir stripping,
      `__MACOSX`/`.DS_Store`/`.git` skipping, unknown-extension
      binary-vs-text sniffing, zip-slip rejection, empty ZIP error,
      invalid-ZIP error, and **export→import round-trip** (zip a fixture
      with `zipSync`, parse it back, assert equality). 20 tests; verified
      they failed before impl.
- [x] Implement `parseProjectZip(zipBytes: Uint8Array): CreateProjectOptions['files']`
      in `ts-packages/quarto-sync-client/src/import-zip.ts`.
- [x] Export it from the package index.
- [x] Typecheck clean; full package suite green (68 tests).

### Phase 2 — preview-runtime wrapper ✅ DONE
- [x] Add `importProjectFromZip(zipBytes: Uint8Array): ProjectFile[]` to
      `ts-packages/preview-runtime/src/automergeSync.ts` (converts
      camelCase → `ProjectFile` snake_case), symmetric with
      `exportProjectAsZip`. Auto-exported via `export * from './automergeSync'`.
- [x] Added shape-conversion + error-propagation tests to
      `automergeSync.test.ts` (29 tests pass; typecheck clean).

### Phase 3 — UI ✅ DONE
- [x] Add "Import from ZIP" button + form to
      `hub-client/src/components/ProjectSelector.tsx` (state, handler,
      file reader, error surfacing) and matching CSS (`.import-btn`).
- [x] Wire submit → `importProjectFromZip` → existing `onProjectCreated`
      (project type passed as the `'imported'` sentinel, which
      `App.handleProjectCreated` ignores). Title prefilled from the ZIP
      filename; sync-server shares the create-form default.
- [x] Component test (`ProjectSelector.import.test.tsx`, jsdom): button
      reveals form, filename prefills title, submit reads file bytes →
      `importProjectFromZip` → `onProjectCreated`, parse errors surfaced,
      submit disabled with no file. 5 tests.
- [x] hub-client typecheck clean; full unit suite green (561 tests).
- NOTE (root-caused): real fflate `zipSync` produces a **corrupt**
  archive under vitest's **jsdom** environment (a one-file zip balloons
  120 B → 518 B and reads back as `index.qmd/`, `index.qmd/0/` …). This
  is a jsdom multi-realm `instanceof` artifact, *not* an fflate bug and
  *not* a different fflate build (the `esm/` node and browser builds
  differ only in the async Worker shim; `zipSync`/`unzipSync` are
  byte-identical):
    1. jsdom installs its own `TextEncoder`; `.encode()` returns a
       `Uint8Array` from jsdom's realm, so `x instanceof Uint8Array`
       (the test realm's global) is **false** (jsdom/jsdom#2524).
    2. fflate's `strToU8` uses `TextEncoder`, so its output is one of
       these foreign-realm arrays.
    3. fflate's `zipSync` flatten (`fltn`, `esm/index.mjs:1653`) decides
       file-vs-directory with `val instanceof u8` (`u8 = Uint8Array`
       captured at module load). The foreign array fails the check, so
       the content bytes are recursed into as a "directory" → the
       `name/0/`, `name/1/` … entries.
  Plain `new Uint8Array([...])` (same realm as the global) is unaffected,
  which is why the binary fixture in the probe round-tripped while the
  `strToU8` text entry did not. Real browsers are single-realm, so
  production is unaffected (proven by the Phase 4 e2e). Consequence: real
  ZIP parsing is tested only in **node-env** suites; the jsdom component
  test mocks `importProjectFromZip`.

### Phase 4 — End-to-end verification (REQUIRED before "done") ✅ DONE
- [x] Add a Playwright e2e in `hub-client/e2e/import-zip.spec.ts` that
      uploads a fixture ZIP and asserts the new project opens in the
      editor with the expected files (mirrors share-link-project-set +
      project-loading specs).
- [x] Real-browser verification via the e2e run (Chromium against a live
      hub) — this *is* the "drive a real browser session" requirement.

**End-to-end evidence (CLAUDE.md policy):**

Invocation:
```
cd hub-client
npx playwright test e2e/import-zip.spec.ts --project=chromium
```
(Playwright's `webServer` serves the production bundle via `vite preview`;
`globalSetup` boots the Rust hub on :3030.)

Fixture ZIP built in-test with fflate: `_quarto.yml`, `index.qmd`
(title "Imported From Zip", body "Hello from an imported zip"), and a
real 1×1 `logo.png` (binary round-trip).

Observed (all assertions passed, `1 passed (12.9s)`):
- After "Import from ZIP" → upload → "Import Project", the URL navigated
  to `/#/p/<localId>` (a new project was created).
- The file sidebar showed **both** `index.qmd` (text) and `logo.png`
  (binary) — confirming binary base64 round-trip survived
  `createNewProject`.
- Opening `index.qmd` rendered, in the preview iframe, the text
  "Hello from an imported zip" and "came from the uploaded archive".

This exercises the real path (real fflate unzip → `importProjectFromZip`
→ `onProjectCreated` → `createNewProject` → Automerge → editor render),
and confirms the fflate/jsdom unit-test quirk does not affect production.

### Phase 5 — Build, changelog, ship
- [x] Production build green: `VITE_E2E=1 npm run build` (tsc -b + vite
      build, the strict gate). Required first rebuilding the dependency
      package dists (`quarto-automerge-schema`, `quarto-sync-client`,
      `preview-runtime`) since vite resolves them via `exports.import`
      → `dist/`, not source.
- [x] TS test gate green: hub-client `test:ci` legs — unit 561,
      integration 66, wasm 79; deps — quarto-sync-client 136,
      preview-runtime 62. (No Rust changes, so the WASM artifact and
      `cargo xtask verify` are unaffected; `build:wasm` would rebuild
      the same bytes from unchanged Rust.)
- [ ] **Pending user go-ahead to commit** — two-commit changelog
      workflow: commit code, then add a `hub-client/changelog.md` entry
      under today's date header with the short hash. Close bd-apv23 +
      `br sync --flush-only` + commit `.beads/`.

## Out of scope (note for follow-ups)
- Importing *into* an existing project (merge/overlay). This plan only
  creates a *new* project.
- Async/worker-based unzip for very large archives (item 7).
- Importing engine capture sidecars / `captures` metadata — only file
  contents are imported; captures regenerate on render.
- Drag-and-drop of a ZIP onto the landing page (could layer on later).

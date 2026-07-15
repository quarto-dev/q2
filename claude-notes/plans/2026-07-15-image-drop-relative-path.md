# Editor image drag-drop: wrong relative path when .qmd is in a subdirectory

**Strand:** bd-jzqswvh0
**Status:** in progress (branch `braid/bd-jzqswvh0-image-drop-relative-path`)
**Date:** 2026-07-15

## Overview

Dropping an image from the desktop onto the markdown editor while editing a
`.qmd` that lives in a subdirectory produces a broken image reference:

- The asset dialog opens with **destination = project root** (`''`), so the
  image is uploaded to e.g. `photo.png` at the root.
- The inserted markdown is `![](<result.path>)` — the *project-root-relative*
  upload path used verbatim. Markdown image paths resolve relative to the
  document's own directory, so from `posts/hello.qmd` the reference resolves
  to `posts/photo.png`, which does not exist.

The two paths only coincide when the `.qmd` is itself at the project root,
which is why the bug hides in the common single-directory case.

## Reproduction (performed 2026-07-15, hub-client dev + public sync server)

1. `npm run dev` in `hub-client/` (defaults to `wss://sync.automerge.org`).
2. Create a new project (`image-drop-path-repro`), create `posts/hello.qmd`,
   open it.
3. Drop a PNG (`photo.png`) from the desktop onto the editor pane.
   (Automated via a synthetic `DragEvent` with a `DataTransfer` holding a
   real `File`, dispatched on `.monaco-editor` — this drives the exact
   `handleEditorDrop` listener a real drop hits.)
4. The "Add asset to project" dialog opens with **Destination folder empty**.
   Click Upload.

**Observed:**
- File sidebar: `photo.png` appears at project root (sibling of
  `_quarto.yml`), not under `posts/`.
- Editor (`posts/hello.qmd`) content: `![](photo.png)`.
- Preview iframe: `<img src="photo.png">` with `naturalWidth: 0` — broken
  image, nothing rendered.

Screenshot: `claude-notes/scratch/image-drop-bug-repro.png`.

## Root cause

`hub-client/src/components/Editor.tsx`:

- `handleEditorDrop` (external-file branch, ~line 854): opens the asset
  dialog with `setAssetDestination('')` — always project root, ignoring the
  current file's directory.
- `handleUploadAsset` (~line 905): inserts
  `` `![](${result.path})` `` where `result.path` is the final
  project-root-relative upload path returned by `createBinaryFile`
  (`CreateBinaryFileResult.path` — authoritative; it can differ from the
  requested path via hash-suffix rename on name conflict). No relativization
  against the current file's directory.
- `handleEditorDrop` (internal sidebar-drag branch, ~line 814): same bug —
  `` `![](${path})` `` / `` `[${fileName}](${path})` `` insert the sidebar's
  project-root-relative path verbatim.

## Design

Two complementary changes:

1. **Correctness (the fix): relativize the inserted path.** Compute the
   inserted markdown path as the relative path from the current file's
   directory to the final uploaded path (`result.path`). Examples, editing
   `posts/hello.qmd`:
   - upload to root `photo.png` → insert `![](../photo.png)`
   - upload to `posts/photo.png` → insert `![](photo.png)`
   - upload to `images/photo.png` → insert `![](../images/photo.png)`
   This holds no matter what destination/filename the user chooses in the
   dialog, and survives the hash-suffix rename (we relativize what was
   *actually* created). This is the "markdown matches the user's chosen file
   name" requirement.

2. **UX default: destination = current file's directory.** For editor drops,
   pre-fill the asset dialog destination with the current `.qmd`'s parent
   directory instead of `''`. The image then lands next to the document and
   the inserted path is the bare filename in the common case. (The sidebar
   drop path already does selection-based defaulting via
   `resolveDefaultDestination`; the editor drop path hardcodes `''`.)

Choice made: use `../`-style relative paths, **not** project-absolute
(`/photo.png`) paths. Rationale: qmd's supported link syntax here is the
plain inline form; relative paths are what the preview's
`resolveRelativePath` (ts-packages/preview-renderer/src/utils/vfsPaths.ts)
resolves against the current file, and they keep documents portable if a
folder is moved wholesale. (Project-absolute paths pass through
`resolveRelativePath` unchanged and would need their own VFS-prefix
handling — a larger change with no user-facing benefit here.)

New helper needed: `relativePathBetween(fromFile, toPath)` — the inverse of
the existing `resolveRelativePath`. Natural home: same module,
`ts-packages/preview-renderer/src/utils/vfsPaths.ts` (hub-client already
imports from `@quarto/preview-renderer/utils/vfsPaths`). It should:

- accept POSIX-style project-root-relative inputs (`posts/hello.qmd`,
  `photo.png`) — the shapes Editor.tsx traffics in;
- walk up with `..` segments as needed;
- be pure and unit-testable.

Also extract the markdown-string construction into a pure, testable
function (e.g. `buildDropMarkdown(currentFilePath, targetPath, kind)` in
`hub-client/src/components/fileUpload/`), covering both the image and the
qmd-link cases, so the Editor callbacks just call it.

## Work items

### Phase 1 — tests first (TDD)

- [x] Unit tests for `relativePathBetween` in
      `ts-packages/preview-renderer/src/utils/vfsPaths.test.ts`:
      same-dir, parent-dir (`../`), sibling-dir, root-file → subdir-doc,
      deep nesting, current file at root (identity), inputs with/without
      leading slash normalization, segment-boundary (`post` vs `posts`).
- [x] Unit tests for the markdown builder
      (`hub-client/src/components/fileUpload/dropMarkdown.test.ts`):
      image vs qmd link, current file at root vs subdirectory,
      hash-suffixed upload path, null current file fallback.
- [x] Unit test for the editor-drop default destination: documented in
      `resolveDefaultDestination.test.ts` ("editor drop" describe block) —
      the editor will pass the current file as `selection`; the mapping
      `posts/hello.qmd → posts` is pure and covered there. The wiring
      itself is Phase 3 E2E.
- [x] Run the new tests; verified failures: 11 × `relativePathBetween is
      not a function`; dropMarkdown suite fails with missing module.
      resolveDefaultDestination additions pass (pure logic pre-exists;
      the bug is the Editor not calling it).

### Phase 2 — implementation

- [x] Add `relativePathBetween` to
      `ts-packages/preview-renderer/src/utils/vfsPaths.ts`.
- [x] Add the markdown-builder helper
      (`hub-client/src/components/fileUpload/dropMarkdown.ts`,
      `buildDropMarkdown(kind, currentFilePath, targetPath)`) and use it
      from `handleUploadAsset` (external drop → post-upload insertion).
      `handleUploadAsset` deps now include `currentFile`.
- [x] Fix the internal sidebar-drag insertion with the same helper
      (images *and* qmd links).
- [x] Default the editor-drop asset-dialog destination to the current
      file's parent directory via
      `resolveDefaultDestination({ selection: currentFile?.path })`.
      Note: safe against stale closures because `MonacoEditor` uses
      `key={currentFile?.path}` — every file switch remounts the editor
      and re-attaches fresh drop handlers.
- [x] All new/existing unit tests pass: hub-client `test:ci` 129/129;
      preview-renderer 530 passed / 36 skipped.

### Phase 3 — verification

- [x] `cd hub-client && npm run build:all` succeeds (exit 0).
- [x] End-to-end browser verification (2026-07-15, dev server, project
      `image-drop-path-repro`, editing `posts/hello.qmd`), four cases:
      1. **External drop, default destination**: dialog opened with
         destination pre-filled `posts`; upload created `posts/photo.png`;
         inserted `![](photo.png)`; preview `<img>` resolved to a data URI
         with `naturalWidth: 1` (renders).
      2. **External drop, destination overridden to root** (file renamed
         `photo2.png` to dodge the exists-check): inserted
         `![](../photo2.png)`; preview resolved and rendered it
         (`naturalWidth: 1`).
      3. **Internal sidebar drag, image at root**: synthetic
         `application/x-hub-file` drop `{path: 'photo.png', type:
         'image'}` inserted `![](../photo.png)`.
      4. **Internal sidebar drag, qmd link**: `{path: 'guides/notes.qmd',
         type: 'qmd'}` inserted `[notes.qmd](../guides/notes.qmd)`.
- [x] E2E evidence recorded here; screenshots
      `claude-notes/scratch/image-drop-bug-repro.png` (before) and
      `claude-notes/scratch/image-drop-bug-fixed.png` (after).
- [x] No Rust touched (hub-client + ts-packages only) — full
      `cargo xtask verify` not required for this change.

**Observed during E2E, out of scope:** re-dropping a file while the
NewAssetDialog had been opened before showed the previous drop's preview
entry still listed (dialog state persists across open/close). Pre-existing
dialog behavior, unrelated to path handling.

### Phase 4 — bookkeeping

- [ ] Commit (staged; awaiting user approval per pre-commit review policy).
- [ ] hub-client changelog entry (two-commit workflow, needs commit 1's hash).
- [ ] `braid close bd-jzqswvh0` with reason.

## Resolved questions (Carlos, 2026-07-15)

1. **Default destination**: current file's directory — confirmed preferred.
2. **Non-image editor drops**: same default applies — one consistent rule.

## Repro artifacts

- Browser project `image-drop-path-repro` (project set of this machine's
  dev profile, public sync server) — reusable for Phase 3 verification.
- Screenshot: `claude-notes/scratch/image-drop-bug-repro.png`

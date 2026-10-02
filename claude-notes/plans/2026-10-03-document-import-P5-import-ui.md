# Plan: Import button, dialog, drop routing and storage (document import P5)

**Date:** 2026-10-03
**Epic:** [`2026-10-03-document-import-epic.md`](2026-10-03-document-import-epic.md) (I5, I11, I12, I19, I20, I21, I22; uses interfaces 3 and 4 and P4's service contract)
**Depends on:** P4 landed. Tasks T1-T4 can start against P4's T0 stub. **Unblocks:** the epic Close-out.
**Branch:** `import/p5-import-ui` from `feature/hub-import`.

## Why

This is everything the user touches:
- the **Import** button next to "Download as" and its file picker;
- the placement dialog;
- drop routing on the file sidebar, the editor and the rest of the window;
- the writes into the Automerge project, with the safety rules: never overwrite (I11), images first and the qmd last (I22), clean up on failure (I22);
- the import report.

## Key files

- Top bar:
  - `hub-client/src/components/DocumentTopBar.tsx` (`.bar-actions`, :123-167; `DownloadAsControl` at :138-147);
  - `DownloadAsControl.tsx`, the pattern for a boxed icon button with a menu anchor (`.qh-icon-btn.boxed`);
  - `Editor.tsx:1594-1617`: `DocumentTopBar` renders whenever the preview isn't fullscreen, with or without an open file; only the `downloadAs` prop is gated (on source files).
- Dialogs:
  - `PlaceFileDialog.tsx`: folder and name; collision check (:82-83) whose error disables confirm (rendered at :132); keyed per file (:54); the name is fixed at mount (`useState(request.name ?? …)`, :76);
  - the place queue in `Editor.tsx:489-496`, rendered at :1889-1895;
  - `NewAssetDialog.tsx`: `FolderPicker`, the drop zone (:238).
- Drops:
  - `FileSidebar.tsx:496-523` (dragover) and `:540-603` (drop), with `folderFromTarget`; with no folder target it uses `resolveDefaultDestination` (:573-579);
  - `collectDroppedEntries` (`hub-client/src/utils/droppedEntries.ts:56`); a top-level file has no `/` in its `relativePath` (:41-47);
  - Monaco DOM handlers `Editor.tsx:1213-1295`, attached at :947-949; the image-position branch at :1279-1284;
  - routing in `Editor.handleDropFiles` (`Editor.tsx:1177-1208`), which calls `processAssetFiles` (:1190, rejects files over 10 MB and empty files);
  - there is no window-level drop handler today.
- Opening a new file: `handleCreateTextFile` (`Editor.tsx:1116-1127`) sets the current file and its content directly. `handleSelectFile` (`:985-1002`) reads from `fileContents`, which a just-created file isn't in yet, and returns early during replay.
- Paths: `resolveDefaultDestination` (`hub-client/src/components/fileUpload/resolveDefaultDestination.ts`), `validateProjectPath`, `sanitizeFilename` (`services/resourceService.ts:124`, turns spaces into hyphens), `joinPath` / `splitName` / `uniquePath` (`hub-client/src/utils/uniquePath.ts:9-24`; `uniquePath` handles one path and suffixes ` 2`, ` 3`, …).
- Writes: `createFile` / `createBinaryFile` / `deleteFile` in `ts-packages/preview-runtime/src/automergeSync.ts:238,246,258`, implemented in `ts-packages/quarto-sync-client/src/client.ts:1610,1634,1692`; `createFolder` / `deleteFolder` at `:1744` and after.
  - `createFile` does not check collisions, and writes the text in a second change after creating an empty doc.
  - `createBinaryFile` returns `{ docId, path, deduplicated }`: `deduplicated: true` and nothing written when the path holds the same hash; a write to `name-<hash8>.ext` when it holds different content.
  - `deleteFile` is keyed by path only.
- Extension sets: `TEXT_EXTENSIONS`, `BINARY_EXTENSIONS` (`ts-packages/quarto-automerge-schema/src/index.ts:478`), `inferMimeType` (:599-629); the Rust mirror `BINARY_EXTENSIONS` in `crates/quarto-hub/src/resource.rs:31` (see the comments at `index.ts:493`, `resource.rs:47`).
- Strings: `hub-client/src/strings.ts` (`download` at :67, `dialogs.*`).
- Tests to copy:
  - `NewAssetDialog.integration.test.tsx`, `DownloadAsControl.integration.test.tsx`;
  - `e2e/import-zip.spec.ts` (`setInputFiles` from a buffer), `e2e/pandoc-download.spec.ts` (real pandoc), `e2e/download-as.harness.spec.ts` with `DevHarness.tsx:986`, `e2e/baseline-a11y.harness.spec.ts`.
  - There are no tests yet for `PlaceFileDialog`, `handleDropFiles` or external drops.

## Decisions local to this plan (within the epic's)

- **The service:** P5 calls only `getImportService()` and `importAvailable()` from `importService.ts` (P4's contract); tests install the stub with `setImportServiceForTests`.
- **One queue.** `PlaceRequest` (`PlaceFileDialog.tsx:21`) gains a variant `{ kind: 'import'; file: File; folder: string }`, and the render site (`Editor.tsx:1889-1895`) renders `ImportDialog` for it and `PlaceFileDialog` otherwise. Import dialogs join the existing place queue as these requests, so a mixed drop never opens a place dialog and an import dialog at once. Each dialog computes its proposal when it reaches the head of the queue, against the index at that moment, so `report.docx` and `report.odt` in one drop propose `report.qmd` and `report 2.qmd`.
- **Folder drops:** only files dropped at the top level are intercepted for import. Files inside a dropped folder are stored as-is, preserving the folder upload's structure.
- **Report display:** when the import finishes, the qmd opens.
  - With no diagnostics, the dialog closes.
  - Otherwise the dialog switches to a report view: diagnostics grouped by kind (errors, warnings, info; a `RustDiagnostic` of kind `note` goes under info). A `RustDiagnostic` shows its title and problem text; a host-style diagnostic (`HostDiagnostic` or `ImportHostDiagnostic`, only a `message`) shows the message. Then Close. Reuse `DownloadAsControl.tsx`'s diagnostic rendering rather than writing a second one. As typed it doesn't accept import diagnostics: `DiagnosticList` (`DownloadAsControl.tsx:36`) and `diagText` (`:29`) take downloadController's closed `Diagnostic` union (`downloadController.ts:26`), and the CSS styles only the kinds Download-as shows. T3 widens them to accept `ImportDiagnostic` too (or moves `diagText` to a shared helper both use), and adds `info`/`note` styling.
  - A failed import shows the same view, with the failure first. A `uiState` from the outcome (loader or worker failure: offline, blocked worker) is shown as the failure, with the same text the Download-as UI uses for that state (`strings.ts`, `download.failed[uiState]`, `:100-118`). Only the loader and worker states (offline, blocked, download failed, unsupported) go through it: its `timeout`, `out-of-memory` and `pandoc-error` texts speak of filters, images and downloads, and P4 never returns those states here.
  - A cancelled import (`cancelled: true`) closes the dialog with no report.
- **Not in the embed, not in replay:** the button and all drop interception exist only when `importAvailable()` is true, and are disabled while `replayState.isActive` (file switching is blocked there).
- **A stored docx** (via Add asset) is a binary file with no viewer: clicking it is a no-op, like pdf and fonts (I21).

## Checklist

### Tasks

- [ ] **T0 Baseline and placement.** Record the hub-client suite counts and the workspace nextest baseline. The top bar renders with no file open and for non-source files (`Editor.tsx:1594`, verified in review); confirm in the dev server that the image viewer state shows it too, and record it.
- [ ] **T1 Extension sets (I21).** Add docx, odt, rtf, epub, pptx, emf and wmf to `BINARY_EXTENSIONS` and their MIME types (P3's format table for the five; `image/emf`, `image/wmf`) to `inferMimeType`, in `quarto-automerge-schema` and in the Rust mirror `crates/quarto-hub/src/resource.rs:31` (which classifies files found on disk). Check first what clicking a stored `.docx` does today and record it, since the epic only inferred it. Tests: after the change, clicking a stored `.docx` is a no-op (no editor opens, no error); the `quarto-hub` classification of a `.docx` and an `.emf` is binary. If a TS/Rust drift test for the two lists exists, extend it.
- [ ] **T2 Import button and picker.** Add an `ImportControl` to `DocumentTopBar`'s `.bar-actions`, next to `DownloadAsControl`. It is a boxed icon button (`aria-label` from `strings.ts`, e.g. "Import document") with a hidden `<input type="file">` whose `accept` is built from `getImportFormats()`: every extension plus every MIME type, comma-separated. The button is passed independently of `downloadAs`, with or without an open file. Choosing a file enqueues an `import` request with `resolveDefaultDestination`'s folder. Hide it in fullscreen preview, like "Download as".
- [ ] **T3 Import dialog.** `ImportDialog.tsx`, modelled on `PlaceFileDialog`:
  - On open, call `validateImportSource(file)` (P4 contract). Q-24-2 over the cap (I19), or Q-24-1 when the user overrode the picker's `accept` filter (it is only advisory), shows Rust's diagnostic text, not a TS string, and disables Import.
  - Folder (`FolderPicker`).
  - Name: proposes `<stem>.qmd`. Build it as `sanitizeFilename(<source name without its extension> + ".qmd")` and split the result: `sanitizeFilename` turns every dot but the last into a hyphen, so sanitizing the bare stem would treat its last interior dot as an extension (`my.report.v2` would become `my-report.v2`). The media folder name derives from the same sanitized stem. If `<stem>.qmd` or the folder `<stem>_media` exists in the destination, the proposal advances to the first free `<stem> 2`, `<stem> 3`, … for both together (I11): a small loop over `joinPath` / `splitName` checking both paths, since `uniquePath` checks one. Computed at mount (see Decisions).
  - A read-only line showing where images will go (`<stem>_media/`).
  - Errors: a typed name colliding with an existing file or media folder; an invalid path (`validateProjectPath`); a validation diagnostic from above. A typed name may contain spaces (P3 percent-encodes image links, I12).
  - The Import button, disabled while any error stands.
  - While importing: progress text per `ImportProgress` stage, with the first-use download's progress from `onLoadProgress` (reuse the `download.*` strings), and Cancel (aborts through the signal). Once `importDocument` has returned and the writes (T6) begin, Cancel and Escape are disabled until the writes finish; the writes take a fraction of a second and stopping them halfway would only trigger cleanup. `ModalDialog` has no locked mode (Escape and the close button both call `onClose`, `ModalDialog.tsx:21-40`), so during the writes `ImportDialog` passes a no-op `onClose`. The request is dequeued only from the report view's Close, or on cancel, not right after Import as `PlaceFileDialog`'s confirm does.
  - Afterwards, the report view (see Decisions).
  - Accessibility like `DownloadAsControl`: focus management, Escape, a live region for progress.
  - Integration tests for each state, with the P4 stub.

  Sizing: two days or more, in two commits, each gated (the dialog's proposal, validation and errors; then progress, the write lock and the report view).
- [ ] **T4 Drop routing (I5).** Put the routing in a pure function, `routeDroppedEntries(entries, { formats, destination })` → `{ imports, uploads }`, in `hub-client/src/utils/`. It detects top-level files by `relativePath` having no `/` and matches extensions case-insensitively against the format table. `Editor.handleDropFiles` and the window fallback both call it. Route importable top-level files to the queue **before** `processAssetFiles` (which would reject a 10-25 MB source), and don't create folders for a drop that held only importable files. The sidebar still calls `expandFolder(destination)` after every drop (`FileSidebar.tsx:584`); that is harmless for imports (the qmd lands there), so leave it.
  - **Sidebar:** destination `folderFromTarget`, or `resolveDefaultDestination` with no target (empty project).
  - **Editor (Monaco):** the current file's folder. A png in the same drop keeps its inserted markdown (`pendingDropPositionRef`).
  - **Window-level fallback (new):** a `useWindowFileDrop` hook that registers `dragover`/`drop` listeners on `document` when `importAvailable()`. The sidebar, Monaco and Add-asset handlers all call `stopPropagation` (`FileSidebar.tsx:499,529,542`; `Editor.tsx:1221,1227,1233`; `NewAssetDialog.tsx:176,182,189`), so their drops never reach `document`. The hook also skips events whose `defaultPrevented` is true. It `preventDefault`s only when `dataTransfer.types` includes `Files`, so the browser never navigates to a dropped file and text drags are left alone. It routes like an editor drop: importable top-level files to the queue with the current file's folder (or `resolveDefaultDestination` with no file open), everything else through `handleDropFiles`. This covers the top bar, the preview pane, the image viewer, no file open, and the overlay of an open dialog (a drop while a dialog is showing joins the queue behind it).
  - The "Add asset" dialog's own drop zone and file input are **not** intercepted (I5, I21).

  Tests: the first drop tests in the codebase. No test mounts `Editor` (Monaco, jsdom), so the cases split three ways:
  - **Unit, `routeDroppedEntries`:** `.DOCX` (upper case); a docx plus a png; a 15 MB docx (routed to import, not to `processAssetFiles`); a folder containing a docx (stored as-is); an unsupported extension (upload, today's behaviour).
  - **Unit, `useWindowFileDrop`** (`renderHook`, events dispatched on `document`): a file drop is routed and `preventDefault`ed; an event already `defaultPrevented` is ignored; a non-file drag is left alone; nothing registers when `importAvailable()` is false.
  - **Integration, the real `FileSidebar`** with a mock `onDropFiles`, `fireEvent.drop` and a `DataTransfer`-like object: a docx on the root, on a folder row, on a file row, in an empty project; a docx plus a png; the Add-asset drop zone (`NewAssetDialog`) storing a docx as-is.
  - The editor, preview-pane, no-file-open and dialog-open cases need the mounted app, so they are T8 Playwright cases.

  Sizing: two to three days, in three commits, each gated: `routeDroppedEntries` and its tests; `useWindowFileDrop` and its tests; the `Editor`/`FileSidebar` wiring and the sidebar integration tests.
- [ ] **T5 Collision-safe writes (I11, I20, I22).** Add `createFileIfAbsent(path, content): Promise<{ created: boolean; docId?: string }>` to `quarto-sync-client` (and the `automergeSync.ts` wrapper). It checks the index for `path` inside the same index change that would add it and refuses if present. The content goes into the new document as **one** change: pass `{ text: content }` as `createDoc`'s initial value instead of `createFile`'s empty doc plus second change. The check-and-add happens inside one `indexHandle.change` callback, which reads `doc.files[path]` and sets a flag outside the closure. After a successful add it does what `createFile` does after its index change (`client.ts:1623-1626`): `subscribeToFileInternal`, `callbacks.onFileAdded`, `tryParseAndNotify`. Folders: `<stem>_media/` needs no explicit marker (folders are derived from file paths); don't call `createFolder` for it. Unit tests in `quarto-sync-client`, in a new test file on the hub-free pattern of `offline-creation.test.ts` (there are no dedicated `createFile` / `createBinaryFile` tests to sit beside, and `restart-window-creation.test.ts` skips silently without `target/debug/hub`), including one that counts the new doc's changes (`Automerge.getAllChanges`) and expects exactly one with the text. The tests must be reported as run, not skipped.

  Residual risk, stated rather than solved: the check sees only the local index. A file another client created at the same path that hasn't synced yet is invisible, and the index map then resolves last-writer-wins. Record this in the code comment.
- [ ] **T6 The import transaction (I22).** `commitImport(outcome, qmdPath, deps)` in `hub-client/src/services/importStorage.ts`, with the sync calls injected so it is testable without a mounted editor. On a successful `ImportOutcome`, keep a list of what this import created, `{ path, docId }`:
  1. For each media entry, in order, `createBinaryFile(projectPath, bytes, mimeType)`. If the result is `deduplicated`, nothing was written: don't record it. Otherwise record `{ result.path, result.docId }` **before** comparing paths, so a renamed write is still cleaned up. If `result.path` differs from `projectPath` (a file with different content appeared there), fail with `import-write-failed`.
  2. Then `createFileIfAbsent(qmdPath, qmd)`, the commit point. If not created, fail with `import-write-failed` ("a file appeared at … while importing").
  3. On any failure or exception: in reverse order, `deleteFile` each recorded path whose index entry still holds the recorded `docId` (read through `getIndexHandle()`, `automergeSync.ts:372`; skip and report any another client has replaced); a deletion error adds `import-cleanup-failed` naming the path. Then show the report view with the failure first, naming the step, path and cause (I22).
  4. On success, open the qmd the way `handleCreateTextFile` does (`setCurrentFile({ path, docId })`, `setContent(qmd)`, plus the URL update `handleSelectFile` makes), not via `handleSelectFile`, and show the report view, or close if there were no diagnostics.

  A crash between steps 1 and 2 leaves only unreferenced images in `<stem>_media/` (I22). Tests of `commitImport` with a fake sync layer: failure at each step with cleanup asserted; a deduplicated media hit not deleted on rollback; a renamed media write deleted on rollback; an entry replaced by another client not deleted. That the opened editor shows the qmd text needs the mounted app: T8 asserts it.
- [ ] **T7 Dev harness and accessibility.** Harness routes for the dialog states (proposal, collision, too large, importing, report, failure, offline `uiState`) in `DevHarness.tsx`, following `#/dev/download-as*`. Add them to `e2e/baseline-a11y.harness.spec.ts`. Add a keyboard spec like `download-as.harness.spec.ts`.
- [ ] **T8 End-to-end.** `e2e/import-document.spec.ts` with real pandoc, in the `pandoc-download.spec.ts` setup:
  - the button path with P1's `basic-docx` via `setInputFiles` (the `import-zip.spec.ts` pattern): the qmd appears in the sidebar and opens with its text, its text contains the expected headings and the image link, the `<stem>_media/` file exists, and the preview shows the image;
  - `track-changes-docx`: the qmd text contains `[++ `, `[-- ` and a span comment `[… [>> …]{author=…}]`. Assert the text only: bubble rendering depends on P0, which the epic removes before the final PR;
  - drops, each by dispatching a synthetic `drop` event with a `DataTransfer` built in `page.evaluate` (`dt.items.add(new File(...))`; `collectDroppedEntries` falls back to `dt.files` when entries are unavailable, `droppedEntries.ts:60-66`):
    - on the sidebar;
    - on the editor (dialog proposes the current file's folder); a docx plus a png on the editor (the png is stored and its markdown inserted);
    - on the preview pane, and with no file open (window fallback, dialog opens);
    - while an import dialog is open (queued behind it);
    - a non-importable file on the preview pane (uploaded, no navigation);
  - a collision: the proposal advances to `<stem> 2`, and the imported `<stem> 2.qmd` shows its image in the preview (percent-encoded link);
  - over-cap: refused before pandoc loads.

  Sizing: about two days (a dozen cases). If a synthetic-drop case is flaky (fails at least once in five runs of `--repeat-each=5`), STOP and record which one and why rather than adding retries.
- [ ] **T9 Changelog.** A `hub-client/changelog.md` entry for Import, in the format the pandoc-wasm phases used (`.claude/hooks/changelog-reminder.sh` prompts for it).

### Verification

- [ ] hub-client typecheck, unit, integration and wasm suites; `quarto-sync-client` and `quarto-automerge-schema` suites; `cargo nextest run -p quarto-hub` (T1's Rust mirror); the Playwright specs above, in every project their config defines: Chromium and Firefox in both `playwright.config.ts` and `playwright.harness.config.ts`, plus WebKit for `{pandoc,typst}-*` harness specs (`playwright.harness.config.ts:66`). CI runs Firefox with `continue-on-error` (`hub-client-e2e.yml:60`), so a Firefox-only failure is recorded, not ignored.
- [ ] Manual check in the dev server, with P0 present: import `track-changes-docx` and see bubbles on the span comments; import `emf-docx` and see the converted image; drop a docx on a folder and on the preview pane. Record the results.
- [ ] Workspace nextest at the plan boundary, delta accounted for.

### Close-out

- [ ] Checklist reconciled, committed; rebased and fast-forwarded into `feature/hub-import`; epic Progress ticked.
- [ ] Confirm no P5 commit edits a file in P0's list (P0's Handoff log).

## Handoff log

Append-only.

- 2026-10-03: plan written. Not started.
- 2026-10-03: revised after review (see the epic's review commit): one queue, window-level drop fallback, images-first write order, cleanup rules, one-change qmd, Rust extension mirror.
- 2026-10-03: revised after the implementability review (epic status line): PlaceRequest import variant; no-op onClose while writing; sanitized stem; routeDroppedEntries + useWindowFileDrop on document; drop cases split unit/integration/Playwright; commitImport in importStorage.ts.
- 2026-10-03: revised after the angle review: stale line numbers; `DiagnosticList` widened for `ImportDiagnostic` with info/note styling; `download.failed` limited to loader/worker states; `createFileIfAbsent` post-steps and its hub-free test file; fixture directory names; sizing of T3, T4 and T8.
- 2026-10-03: rebased onto `feature/pandoc-wasm` `3dfa5b296`; citations re-checked (the hub-client, sync-client, schema and `resource.rs` code P5 cites is unchanged). Verification's Playwright line updated: both configs gained a `firefox` project.

# Generic file uploader dialog for hub-client

- **Beads**: bd-eity
- **Status**: drafted 2026-04-21 — implementation deferred to a separate session
- **Related**: blocks real-browser end-to-end verification for syntax-highlighting Phase 4 (`claude-notes/plans/2026-04-21-syntax-highlighting-phase-4.md` step 4.6).

## Why this plan exists

Hub-client currently supports dropping image files onto the editor or FileSidebar, which routes through `NewFileDialog` → `resourceService.processFileForUpload` → `automergeSync.createBinaryFile` → VFS. The ingestion pipeline is already generic (binary bytes, SHA-256, MIME detection, Automerge store, VFS propagation). What's **not** generic:

- `Editor.tsx:767` filters drops to `image/*` only — PDFs, `.wasm`, CSVs, arbitrary binaries fall through.
- The `<input type="file">` in `NewFileDialog.tsx:492` restricts `accept="image/*,.pdf,.svg"`.
- `NewFileDialog` is primarily a "create new file" affordance; uploading an existing binary feels like a secondary code path.

Project authors already need to put arbitrary binary files into a project: images, PDFs, CSVs, `.wasm` tree-sitter grammars (Phase 4), data files, custom fonts, SVGs, and so on. The current UI forces most of these through an unnatural path or requires out-of-band mechanisms.

## Goals

1. **One generic "add files to project" affordance** — a dialog triggered from a consistent entry point (e.g. a "+" button in FileSidebar, an "Add files…" menu item), plus drag-and-drop anywhere meaningful in the UI.
2. **Any binary file type** — accept anything up to the existing 10 MB size cap (`resourceService.ts:165`). No MIME allow-list beyond obvious sanity (e.g. reject completely empty files).
3. **Destination path selection** — user can pick where in the project tree the file(s) land. Defaults to a sensible location (e.g. project root or currently focused folder in FileSidebar).
4. **Works for the concrete cases we care about today**:
   - Image assets (existing flow preserved).
   - Tree-sitter grammar bundles: `.wasm` + `.scm` pairs landing in `_quarto/grammars/<name>/` (Phase 4 testability).
   - PDFs, CSVs, fonts, other binaries that already "should" work.
5. **Doesn't break the editor's markdown-insertion-on-drop UX**. Dropping an image inside the editor still inserts a markdown image reference at the drop point. Dropping a non-image triggers the upload flow but does **not** insert a reference.

## Non-goals

- Server-side validation / virus scanning / size-cap increases. 10 MB stays, content is trusted (hub-client is an authoring tool, not a public upload endpoint).
- Multi-select-then-batch-rename UX, thumbnails in the picker, etc. Phase 2 polish if it turns out to matter.
- Any grammar-specific UI. The uploader is the generic primitive; grammar discovery scans `_quarto/grammars/` regardless of how the files got there.

## Current state (2026-04-21 survey)

Evidence for the "it's mostly refactoring" framing:

- Drag-drop handlers: `FileSidebar.tsx:142-154`, `Editor.tsx:718-786`, `NewFileDialog.tsx:174-200`.
- Binary upload pipeline: `resourceService.ts:70` (`processFileForUpload`), `automergeSync.ts:201-208` (`createBinaryFile`), `automergeSync.ts:86-88` (VFS callback).
- File enumeration for default destination: `App.tsx:69`, `fileTree.ts:50+`, `buildFileTree()`.
- Modal primitive pattern (no external dialog library): `NewFileDialog.tsx:337-530`, `ShareDialog.tsx` as a simpler sibling.
- File-picker element: `NewFileDialog.tsx:488-495` — hidden `<input type="file" multiple accept="image/*,.pdf,.svg">`.
- Size cap: `FILE_SIZE_LIMITS.MAX_FILE_SIZE = 10MB` (`resourceService.ts:165-167`).

## Proposed design

### Dialog component: `NewAssetDialog`

Introduce a new sibling dialog component, `NewAssetDialog.tsx`, and keep `NewFileDialog` for the "new file to be edited via Monaco" case. Rationale: the two flows are conceptually different — `NewFileDialog` creates a file the user will edit in-editor (templates, filename picking, text-content seeding), while `NewAssetDialog` ingests opaque binary assets the editor doesn't interpret (images, PDFs, `.wasm`, data files). Keeping them separate preserves the template/filename UX of `NewFileDialog` without bloating it, and it lets us trim the asset dialog down to what upload actually needs.

Components:

- **Header**: "Add asset to project"
- **Body**:
  - Drop zone (same styling as existing drag-drop zone) accepting any MIME.
  - "Browse…" button opening `<input type="file" multiple>` with **no `accept` filter**.
  - Destination picker: text input plus "browse tree" selector, seeded from the destination-derivation logic (see below). Defaults to project root (`""`) when no better hint is available.
  - File list preview: each selected file's name, size, MIME; user can remove individuals from the list before confirming.
- **Footer**: Cancel / Upload buttons. Upload triggers `processFileForUpload` → `createBinaryFile` for each asset using the composed path `<destination>/<filename>`.

Code reuse: both dialogs share `processFiles()`, `sanitizeFilename()`, and the file-preview row UI. Lift those pieces into a small shared module (`components/fileUpload/`) so `NewAssetDialog` doesn't re-implement them.

### Destination derivation (shared logic)

Today `FileSidebar.handleDrop` (FileSidebar.tsx:142-154) does **not** pick a destination based on drop target — it passes all dropped files up through `onUploadFiles(droppedFiles)` with no location hint, and the resulting upload always lands at project root. That's a gap we should close as part of this work, because the same derivation logic needs to feed both drag-drop and "+"-button-initiated flows.

Approach: extract a `resolveDefaultDestination(opts)` helper that returns a folder path (possibly `""` for root) given:

1. **Drop target** (if a drop event): walk up from `event.target` to the nearest folder node (via a `data-folder-path` attribute on folder headers and file rows in FileSidebar). Files contribute their parent folder; folders contribute themselves.
2. **Current selection fallback**: if there's no drop target, use the currently focused file's parent folder (`currentFile.path`'s dirname).
3. **Root fallback**: if neither is available, return `""` (project root).

To make #1 work we need to tag tree rows with their folder association (one-line change in `renderFileItem` and `renderTreeNode`). This is a small, contained refactor and is worth doing in Phase A before the dialog lands, because it makes the "+"-button flow and the drop-flow trivially consistent.

### Trigger points

- "+" icon button in FileSidebar header → opens `NewAssetDialog` with `destination = resolveDefaultDestination({ selection: currentFile })`.
- Optional: menu bar "Add asset…" → opens dialog with destination = project root.
- Drag-drop **on the sidebar** → opens dialog pre-populated with the dropped files, `destination = resolveDefaultDestination({ dropTarget: event.target, selection: currentFile })`.
- Drag-drop **on editor** → *all* file drops (image and non-image) route through `NewAssetDialog` with destination = project root. For image drops, `Editor.tsx` stashes the drop position in `pendingDropPositionRef` as it does today; after `NewAssetDialog` triggers `onUploadAsset`, the editor's callback handles `createBinaryFile` + markdown-at-drop-point insertion. Non-image editor drops get the upload with no markdown insertion. This consolidation is deliberate: an asset dropped into a qmd buffer is an asset, not a "new file to edit," so routing through the asset path is more honest.

With editor drops migrated to `NewAssetDialog`, `NewFileDialog` can shed its "Upload File" tab entirely and revert to text-only (filename + template). That simplification is part of this plan's Phase C.

### Validation & limits

Implement as a `validateProjectPath(path)` helper in the shared upload module, applied both to destination paths and to composed `<destination>/<filename>` strings before calling `createBinaryFile`. The existing `createBinaryFile` in `ts-packages/quarto-sync-client/src/client.ts:569` performs **no path validation** — it writes `doc.files[path] = docId` as-is, which is how leading-`/` bugs have slipped through. We enforce at the UI layer instead.

Rules:

- **Reject leading `/`.** Today `createBinaryFile` silently accepts `/foo.png`, producing a VFS key that doesn't match any lookup (callers strip leading slashes inconsistently; see `Preview.tsx:165`, `iframePostProcessor.ts:121`, `ReactAstSlideRenderer.tsx:845`). This has caused real issues. Normalize by rejecting at input.
- **Reject `.` and `..` path segments.** Split on `/`; no segment may equal `.` or `..`. This prevents both accidental path traversal and `./foo.png`-style noise.
- **Reject forbidden filename characters** (already in `NewFileDialog.tsx:221` for filenames): `<>:"|?*\`. Apply the same check to each path segment.
- **Reject empty segments** (i.e. `foo//bar`).
- **Reject empty files** (sanity).
- **Size cap**: existing 10 MB cap enforced in dialog pre-upload (same as `NewFileDialog.tsx:68-74`); keep.
- **Name collision**: if the composed `<destination>/<filename>` already exists, reject with a clear error (same as existing `validateUploadFilename`, extended for destination).

No progress UX for large uploads in this iteration. 10 MB cap keeps uploads fast enough that it's not worth the complexity yet.

### Non-mutations to the ingestion pipeline

`processFileForUpload`, `createBinaryFile`, and the VFS callback path need no changes. This plan is almost entirely UI-layer. (The path-validation helper sits in the UI layer, not in `createBinaryFile` itself — that's a deliberate scope call for this plan, since the sync-client API is shared with other entry points and tightening it there is a separate decision.)

## Test-first approach (per CLAUDE.md TDD rule)

1. **Component test: `NewAssetDialog` opens, accepts files, lists them** — Vitest + @testing-library/react. Drop a fake File list into the dialog; assert it appears in the preview.
2. **Component test: destination path defaults and validation**.
   - Default destination seeded from props (simulates `resolveDefaultDestination` output).
   - Reject leading `/`.
   - Reject `.` and `..` segments.
   - Reject forbidden chars (`<>:"|?*\`).
   - Reject empty segments (`foo//bar`).
   - Successful upload with a nested destination (`_quarto/grammars/toml/`).
3. **Unit test: `resolveDefaultDestination`**.
   - Drop target on a file row → parent folder.
   - Drop target on a folder header → that folder.
   - No drop target, selection on a file → selection's parent folder.
   - No drop target, no selection → `""` (root).
4. **Integration test: upload flow**. Fake the Automerge layer (existing patterns in hub-client tests). Drop a `.wasm`-like file with destination `_quarto/grammars/toml/`; assert `createBinaryFile` called with path `_quarto/grammars/toml/<name>.wasm` and correct bytes.
5. **Editor drop test: non-image file routes to `NewAssetDialog`, does not insert markdown reference**.
6. **Editor drop test: image drop routes to `NewAssetDialog` and still inserts markdown reference at drop point** (the insertion logic moves from `NewFileDialog`'s upload callback to `NewAssetDialog`'s equivalent callback; `pendingDropPositionRef` still drives where the insertion lands).
7. **Regression test: `NewFileDialog` text/template flows unchanged** after the upload tab is removed (filename input, template selection, create-text-file path all still work).

## Work items

### Phase A — Shared foundations

- [x] Extract `components/fileUpload/` shared module: `processAssetFiles`, `validateProjectPath`, `resolveDefaultDestination`. (`FilePreview` row deferred to Phase B where it's actually used.)
- [x] Implement `validateProjectPath` (leading `/`, `.`/`..` segments, forbidden chars, empty segments). Unit tests first. (25 tests pass.)
- [x] Implement `resolveDefaultDestination({ dropTarget?, selection? })` helper. Unit tests first. (12 tests pass.)
- [x] Tag FileSidebar tree rows with `data-folder-path` on folder headers and file rows so `resolveDefaultDestination` can read the drop target.
- [x] Keep existing `NewFileDialog` behavior intact (integration test still green; no NewFileDialog code touched in Phase A).

### Phase B — `NewAssetDialog`

- [x] Create `NewAssetDialog.tsx` using the shared module.
- [x] File input with **no `accept` filter**.
- [x] Destination-path input seeded from `defaultDestination` prop; live validation via `validateProjectPath`.
- [x] File preview list with per-file remove (aria-labelled remove buttons).
- [x] Wire up Cancel / Upload; Upload composes `<destination>/<filename>`, validates once more, then calls the `onUploadAsset` callback (caller handles `processFileForUpload` → `createBinaryFile`).
- [x] Reject empty files (size === 0).
- [x] Vitest coverage for component behavior (18 integration tests pass).

### Phase C — Trigger points

- [x] Add "Upload" button to FileSidebar header that opens `NewAssetDialog` with `defaultDestination = resolveDefaultDestination({ selection: currentFile?.path })`.
- [x] Sidebar drop routes through `resolveDefaultDestination({ dropTarget, selection })` → `NewAssetDialog`. Destination passed up via `onUploadFiles(files, destination)` signature.
- [x] Editor drop handler: *all* file drops (image and non-image) route to `NewAssetDialog`. Image drops continue to stash `pendingDropPositionRef`; markdown-at-drop-point insertion fires from `handleUploadAsset`.
- [x] Remove the "Upload File" tab from `NewFileDialog` and all now-unused state (mode switching, file-preview logic, drop zone, `accept` filter). Dialog is now text-only.
- [x] Update `NewFileDialogProps` to drop `onUploadBinaryFile` and `initialFiles`; update callers in `Editor.tsx`.
- [x] Added `FileSidebar.integration.test.tsx` covering Upload-button destination derivation (4 tests).
- [ ] Optional menu-bar "Add asset…" entry. (Deferred — not required for the core feature.)

### Phase D — Verification

- [x] `cd hub-client && npm run test && npm run test:integration` green: 502 unit tests + 35 integration tests pass (up from 502 + 13 baseline).
- [x] `cd hub-client && npm run build:all` green. WASM bundle built; production Vite build succeeds.
- [x] `cargo build --workspace` green (no Rust regressions from WASM-adjacent work).
- [x] `npx tsc --noEmit` green.
- [~] `npm run test:ci` — the unit and integration steps pass. `npm run test:wasm` has **one pre-existing failure** in `src/services/smokeAll.wasm.test.ts` (`highlighting/03-user-grammar/03-user-grammar-toml.qmd` fails the `<pre class="sourceCode toml"` regex). Verified this failure reproduces at HEAD with my changes stashed — it is **unrelated to the file-uploader work** and appears to be a carryover from the in-progress syntax-highlighting Phase 3.5 work on the same branch. Not fixing it here.
- [ ] **Manual browser session**: **not performed in this session.** The Claude-in-Chrome extension is not connected, so I could not drive a real browser. The dev server does start cleanly (`npm run dev` listens on localhost:5173). A human-operator manual pass is still required before the feature ships: upload the TOML grammar fixture (`.wasm` + `.scm`) into `_quarto/grammars/toml/`; confirm both appear in FileSidebar under the correct folder; confirm Phase 4's syntax highlighting picks them up (once Phase 4 lands). Record the click-path here when performed.
- [ ] **Leading-`/` rejection sanity-check**: covered by unit tests (`validateProjectPath.test.ts` and `NewAssetDialog.integration.test.tsx` both assert rejection), **not** exercised end-to-end through the real upload path. A manual pass should confirm that typing `/foo.png` into the destination input disables the Upload button with a visible error.

### Verified end-to-end (what testing proves)

- `validateProjectPath` rejects leading `/`, `.`/`..` segments, forbidden chars, and empty segments (25 unit tests).
- `resolveDefaultDestination` picks the right folder from drop target, selection, or root fallback (12 unit tests).
- `NewAssetDialog` renders, ingests files, validates destination and per-file paths live, composes `<destination>/<filename>` correctly, calls `onUploadAsset` once per valid file, blocks upload when destination is invalid, rejects collisions, and calls `onClose` on success (18 integration tests).
- `NewFileDialog` still works in text-only mode (12 of 13 integration tests; 1 new test verifies the upload tab is gone).
- `FileSidebar` Upload button derives destination from the current selection or falls back to root (4 integration tests).
- `processAssetFiles` rejects empty and oversized files (5 unit tests).

### Not covered by automated tests (requires real browser)

- Editor-drop image uploads: inserting `![](path)` at the drop position. The logic is preserved (the markdown-insertion code moved from `handleUploadBinaryFile` to `handleUploadAsset` unchanged), but without Monaco in tests we don't exercise the actual editor integration.
- Sidebar drag-and-drop derivation from `data-folder-path` of the drop target element. Unit-tested in `resolveDefaultDestination.test.ts` with a simulated DOM, but not through a real drag event.
- Actual VFS and Automerge propagation after `createBinaryFile`. Existing behavior; no changes to the sync pipeline in this plan.

## Resolved decisions (2026-04-21)

1. **Sibling dialog, not refactor-in-place.** `NewFileDialog` becomes text-only (filename + template); the new `NewAssetDialog` handles all binary-asset ingestion. Shared pieces live in `components/fileUpload/`.
2. **Default destination for "+" with no selection: project root** (`""`). The rename flow already works fine on the project-folder view, so no need to remember last-used.
3. **Share destination-derivation logic between drop and "+"-button paths.** Current FileSidebar drop handler does *not* inspect the drop target — it needs that capability added (Phase A). `resolveDefaultDestination` is the single source of truth for both entry points.
4. **Path validation is in-scope for this plan.** `createBinaryFile` in the sync-client does no validation today; we add `validateProjectPath` in the UI layer, rejecting leading `/`, `.`/`..` segments, empty segments, and the existing forbidden-char set. Leading-`/` rejection closes a known small bug category.
5. **No progress/error UX for large uploads** in this iteration. 10 MB cap keeps this simple.
6. **All editor drops route through `NewAssetDialog`.** Image drops retain the markdown-at-drop-point insertion via `pendingDropPositionRef` and the new `onUploadAsset` callback. `NewFileDialog` loses its upload tab as a result.

## Open questions for the implementing session

(none currently — answers from the 2026-04-21 review are folded into the sections above. Add new items here as they surface during implementation.)

## Dependencies

- Blocks: syntax-highlighting Phase 4.6 (real-browser end-to-end verification of user grammars).
- Blocked by: nothing.

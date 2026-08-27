# Paste images from clipboard into the Monaco source editor

**Braid strand:** bd-706b0ixu
**Created:** 2026-08-27
**Status:** Approved — executing (design decisions resolved 2026-08-27)
**Follow-up strands:** bd-yspyic32 (mixed text+image payloads), bd-myoj9kp5
(pipeline-wide SVG posture)

## Decision log (2026-08-27, user-approved)

1. **Precedence:** v1 uses the simple file-only rule (§2a, as refined
   below): take over iff the payload's files are all accepted raster
   images **and** `text/plain` is empty or is just the filename rider.
   Payloads with meaningful text (Office, Excel/Sheets) paste as text;
   offering their image rendition is bd-yspyic32.
2. **Filename:** `pasted-<hash8>.<ext>` (8-char SHA-256 prefix).
3. **Destination:** P1 — same directory as the current document.
4. **SVG:** S1 confirmed — raster-only paste; SVG stays dialog-only.
   Pipeline-wide SVG decision filed as bd-myoj9kp5.
5. **Multi-file separator:** spaces (indentation-sensitive contexts rule
   out newlines; see §D4).
6. **Declined file-only pastes:** keep status quo (Monaco inserts the
   filename as text); revisit after real-world usage.

## Overview

Add clipboard-paste support for images in the hub-client source editor
(Monaco), analogous to the existing drag-and-drop flow but with one key UX
difference: **no dialog**. On paste we automatically pick a filename, create
the binary file in the project, and insert a markdown image reference at the
cursor, so the image shows up in the rendered preview as soon as the paste is
processed. Because there is no user-chosen filename, the scheme must be safe
against another peer *concurrently* pasting an image into the same CRDT
session.

This document covers: (1) what the existing code gives us, (2) a primer on
how clipboard images actually arrive in JavaScript, (3) the security
analysis (SVG in particular), and (4) the design options with a
recommendation.

## 1. What already exists (survey of current code)

The drag-and-drop feature (plan: `2026-01-10-monaco-image-drag-drop.md`,
generic uploader: `2026-04-21-generic-file-uploader.md`) built almost all the
machinery we need:

- **`Editor.tsx`** (`hub-client/src/components/Editor.tsx:814-965`)
  - `handleEditorDrop` attaches to the Monaco container's DOM node,
    distinguishes internal sidebar drags (`application/x-hub-file`) from
    external file drops, stashes the drop position in
    `pendingDropPositionRef`, and routes external files to `NewAssetDialog`.
  - `handleUploadAsset` runs `processFileForUpload` → `createBinaryFile`,
    then inserts markdown at the pending position via
    `editorRef.current.executeEdits(...)`. Monaco's `onChange` fires
    synchronously from `executeEdits` and propagates to the CRDT via the
    splice path — so text insertion needs no extra sync work.
  - The editor sets `pasteAs: { enabled: false }` in Monaco options
    (quarto-dev/kyoto#3) — this disables Monaco's paste-as *widget* for text
    pastes; it does not conflict with a DOM-level paste listener.
- **`fileUpload/dropMarkdown.ts`** — `buildDropMarkdown('image',
  currentFilePath, targetPath)` produces `![](href)` with the target
  correctly relativized against the containing document (bd-jzqswvh0).
- **`fileUpload/resolveDefaultDestination.ts`** — destination defaults to
  the current file's parent folder.
- **`services/resourceService.ts`** — `computeSHA256`, `getHashPrefix`,
  `generateHashedFilename` ("diagram.png" → "diagram-a1b2c3d4.png"),
  `sanitizeFilename`, `processFileForUpload` (reads bytes, hashes, resolves
  MIME from `file.type` falling back to extension), and
  `FILE_SIZE_LIMITS.MAX_FILE_SIZE` (10 MB).
- **`quarto-sync-client/src/client.ts:1278` — `createBinaryFile(path,
  content, mimeType)`** — the CRDT write. Already conflict-aware, per-client:
  - If `path` exists **with the same SHA-256** → returns
    `{ deduplicated: true }` and creates nothing.
  - If `path` exists **with different content** → renames to
    `name-<hash8>.ext` and creates a new doc.
  - Then sets `doc.files[path] = docId` in the index document (an Automerge
    map — concurrent writes to the *same key* resolve last-writer-wins).
- **Preview side** (`ts-packages/preview-renderer`): `assetWalker.ts` mints
  blob URLs for project-relative image targets; `inlines/Image.tsx` renders
  them as `<img src={blobUrl}>` inside the preview iframe
  (`sandbox="allow-scripts allow-same-origin"`). The asset manifest updates
  reactively when files are added, so a newly created binary file becomes
  visible in the preview as soon as the AST references it.

**Notable pre-existing fact:** the drop path already accepts SVG — every
image check is `file.type.startsWith('image/')`, and `image/svg+xml`
matches. So paste does not *introduce* the SVG question; it inherits it (see
§3).

## 2. Clipboard images in JavaScript: formats and APIs

There are two browser APIs; only one is right for this feature.

### 2a. The `paste` event (`ClipboardEvent.clipboardData`) — the right one

Fires on the focused element when the user presses Cmd/Ctrl-V. Synchronous,
no permission prompt, and hands us a `DataTransfer` — the same interface the
drop handler already consumes (`items`, `files`, `types`, `getData`). In
Monaco, the event target is the hidden `<textarea>` (`inputarea`) inside the
editor DOM node, so a **capture-phase listener on
`editor.getDomNode()`** sees it before Monaco does and can
`preventDefault()` + `stopPropagation()` when we take over.

What actually arrives, by source of the copy:

| User action | `clipboardData` contents |
|---|---|
| OS screenshot to clipboard; "copy" in an image editor; Chrome "Copy image" on a web page | A file item of type **`image/png`** (browsers re-encode bitmap clipboard content to PNG), usually alongside a `text/html` string (`<img src="...">`) |
| Copy a *file* in the OS file manager (Finder, Explorer) | The file itself in `clipboardData.files`, with its **real MIME type**: `image/svg+xml`, `image/jpeg`, `image/gif`, `image/webp`, `image/avif`, `application/pdf`, … Often *also* a `text/plain` item carrying the filename — this is why an unhandled file paste inserts stray filename text into the editor |
| Copy from Office/Google Docs | `text/html` + `text/rtf` + usually an `image/png` rendition |
| Copy SVG *markup* from a text editor | Plain `text/plain` — an ordinary text paste; not our concern (equivalent to typing it) |

Practical consequences:

- **PNG is the lingua franca.** Every "copied pixels" source arrives as
  `image/png`. This is the dominant use case (screenshots).
- **Non-PNG raster and SVG arrive only via file-copy paste**, with correct
  MIME types on the `File` objects.
- **Mixed payloads need a precedence rule.** When both text and an image
  file are present (Office copy), pasting the image is usually wrong —
  the user expects their text. Rule of thumb (matches VS Code's behavior):
  if `clipboardData` contains a non-empty `text/plain` item **and** the
  source also provided `text/html`, treat it as a text paste; take over
  only when the payload is file-only, or when the only text is the
  filename that rides along with a file copy (detectable: file present +
  text equals the file's name). **Decided v1 rule: take over iff
  `clipboardData.files` is non-empty, every file is an accepted raster
  image type, and the `text/plain` item is empty or is merely the
  filename rider (equals a pasted file's name, or the newline-joined
  names); else let Monaco handle it.** The text guard matters: Excel /
  Sheets cell copies carry an `image/png` rendition of the cells *plus*
  the `text/plain` TSV — without the guard, pasting cells would insert a
  screenshot of them. (Chrome's "Copy image" case includes `text/html`
  but *no* `text/plain`, so it still routes to us.) Offering the image
  rendition of mixed payloads is follow-up bd-yspyic32.

### 2b. The async Clipboard API (`navigator.clipboard.read()`) — not needed

Requires a permissions prompt and a user gesture, and Chromium supports
reading only `text/plain`, `text/html`, and `image/png` (images are
sanitized/transcoded to PNG). It's the right tool for a toolbar "Paste
image" *button* (no paste event available), but unnecessary for Cmd-V. Not
proposed for v1; noted as the extension point if we ever add a button.

### 2c. Monaco-native hooks — insufficient

- `editor.onDidPaste` fires only after a **text** paste lands; no file
  access. Useful for nothing here.
- VS Code's `DocumentPasteEditProvider` (how vscode itself does
  paste-image-into-markdown) is **not exposed in monaco-editor 0.55.1's
  public API** (verified against `editor.api.d.ts` — no `DocumentPaste` /
  `PasteEdit` symbols). So the DOM listener is the only viable
  interception point, mirroring how the drop feature already works.

### 2d. What Monaco does when we pass through (verified, monaco 0.55.1)

Read from the shipped sources (`esm/vs/editor/browser/controller/editContext/
clipboardUtils.js`, `.../textArea/textAreaEditContextInput.js`):

- Monaco's own paste handler on the hidden textarea calls
  `e.preventDefault()` unconditionally and re-implements insertion. It
  reads **only** `text/plain` and its private `vscode-editor-data` JSON
  flavor (copy metadata: language mode, multicursor segments,
  empty-selection flag). `text/html`, `text/rtf`, and file *contents* are
  never read.
- **Filename fallback:** if `text/plain` is empty, there is no
  vscode metadata, and `clipboardData.files` is non-empty, Monaco inserts
  the file **names** joined by newlines (`clipboardUtils.js:69-73`). So
  today a Finder-copied `diagram.svg` pastes the text `diagram.svg`, and a
  screenshot pastes something like `image.png`. Not a no-op.
- If text is still empty and there are no files: early return, nothing
  happens.
- Matching self-copied text restores multicursor/paste-on-new-line
  behavior via an in-memory store; irrelevant for external payloads.
- The `dropOrPasteInto`/`pasteAs` contribution (VS Code's "Paste As..."
  provider machinery) is disabled in this codebase (`pasteAs: { enabled:
  false }`, kyoto#3), so none of its built-in providers run.

Implications: the Office mixed payload passes through to exactly the right
outcome (text inserted, PNG rendition ignored). But a *declined* file-only
paste (e.g. an unsupported `image/tiff`, or a PDF) falls through to
filename-text insertion — today's behavior, but worth deciding whether a
toast would serve better (see open question 6).

## 3. Security analysis

### 3a. Is SVG an "image" here, and what can it do?

Yes — `image/svg+xml` satisfies every `startsWith('image/')` check in the
current code, and `inferMimeType`/`guessMimeType` map `.svg` to it. An SVG
can contain `<script>`, event handlers (`onload=`), `<foreignObject>` with
arbitrary HTML, and external references. Whether any of that *executes*
depends entirely on the embedding context:

| Context | Scripts execute? |
|---|---|
| `<img src="x.svg">`, CSS `background-image` | **No.** Browsers render SVG-as-image in "secure static mode": no scripts, no external loads, no interactivity. |
| `<object>`, `<embed>`, `<iframe>` pointing at the SVG | Yes. |
| **Navigating directly to the SVG URL** (e.g. right-click → "Open image in new tab") | Yes — it becomes an SVG *document*, and scripts run with the URL's origin. |
| Inlining the SVG markup into the DOM | Yes. |

### 3b. Where pasted-SVG bytes could execute in *our* pipeline

1. **Hub preview:** `Image.tsx` renders `<img src={blobUrl}>` — safe in
   situ. But the blob URL is minted by `URL.createObjectURL` **in the
   parent hub-client context**, so its origin is the hub-client origin. A
   user who opens that blob URL directly gets a script-capable SVG
   document running on the hub origin. Caveat on severity: the attacker
   here is a *collaborator with write access to the project*, who can
   already put raw HTML blocks into a qmd document that the preview
   renders — so pasted SVG does not obviously cross a trust boundary the
   collaborative editor hasn't already crossed. Worth a deliberate
   decision, not an accident.
2. **Rendered/published site (`q2 render`):** the SVG is copied verbatim
   into the output and served by whatever hosts the site as
   `image/svg+xml`. `![](x.svg)` becomes `<img>` (safe), but anyone
   navigating to `https://site/x.svg` directly executes its scripts on the
   site's origin. This is the standard static-site-generator posture
   (same as committing an SVG to any repo and publishing it) — not
   something the paste feature can or should solve globally.
3. **Hub HTTP endpoints:** if the hub server ever serves raw project files
   over HTTP (downloads, published previews), SVG responses should carry
   `Content-Security-Policy: sandbox` and/or `Content-Disposition:
   attachment` — the industry-standard mitigation (GitHub serves user SVGs
   from a sandboxed domain with CSP). Out of scope for this feature but
   should be recorded as a hub-server follow-up strand if such an endpoint
   exists or appears.

### 3c. Policy options for pasted SVG

- **(S1) Raster-only paste allowlist** — accept `image/png`, `image/jpeg`,
  `image/gif`, `image/webp` (optionally `image/avif`) on the *paste* path;
  any other file type falls through to the existing asset-dialog flow (or
  is ignored with a toast). SVG thus stays available via the deliberate,
  visible upload/drop dialog — exactly the status quo — while the
  *silent, no-dialog* path never ingests active content. Zero new
  dependencies, zero lossy transforms.
- **(S2) Sanitize SVG on paste** (DOMPurify with SVG profile) — keeps
  vector pastes working but adds a dependency, and sanitizers are a
  moving target; also mutating the user's bytes silently is surprising
  (hash changes, diffs against the original file).
- **(S3) Rasterize SVG on paste** (draw to canvas, export PNG) — silently
  destroys vector quality; worst option for a Quarto audience.
- **(S4) Accept SVG as-is on paste** — consistent with drag-and-drop
  today, but expands the silent path to active content.

**Recommendation: S1.** It is the smallest surface, matches the "paste =
screenshot" dominant use case (which is always PNG), and doesn't regress any
current capability — SVG ingestion remains available where it is today, in
the dialog flows. Separately, file a strand to make the *deliberate* SVG
decision for the whole upload pipeline (preview blob-URL origin, future hub
serving endpoints), since that pre-exists this feature.

Two non-issues worth recording: (a) pasting SVG *markup* as text is an
ordinary text edit, indistinguishable from typing it; (b) browser clipboard
bitmap sanitization means we never receive raw platform bitmap formats —
the browser hands us well-formed PNG.

## 4. Design decisions and options

### D1. Interception point

DOM `paste` listener, **capture phase**, on `editor.getDomNode()` —
registered/removed exactly where the drag-drop listeners already are
(`Editor.tsx` mount effect + the existing cleanup effect). Handler logic:

```
on paste(e):
  files = imagesOnly(e.clipboardData)        # per §2a precedence rule
  if files is empty: return                  # Monaco handles text normally
  e.preventDefault(); e.stopPropagation()    # stop filename-text insertion
  for each file: ingest(file)                # §D2–D5
```

### D2. Filename scheme (the no-dialog core decision)

Requirements: automatic, collision-safe against concurrent peers, stable
enough that repeat-pastes of the same content don't proliferate files.

- **(F1) Content-hash name: `pasted-<hash8>.<ext>`** (ext from MIME).
  - Concurrent pastes of *different* images by two peers → different
    hashes → **different index keys → no CRDT conflict at all**. This is
    the crucial property: `createBinaryFile`'s existence check is
    check-then-act on the *local* replica, so two peers concurrently
    claiming the same path with different content would race to
    last-writer-wins on the index map key and one image would silently
    vanish. Distinct names sidestep LWW entirely.
  - Concurrent pastes of the *same* image → same name, same content; LWW
    picks one docId, both peers' markdown references resolve to identical
    bytes. The losing doc is orphaned (unreferenced in the index) —
    harmless. Sequentially, the dedup branch already returns
    `deduplicated: true` and creates nothing.
  - Repeat paste of the same screenshot by one user → dedup, single file.
  - Uses `computeSHA256` we already run in `processFileForUpload` — the
    hash is computed anyway; no extra cost.
- **(F2) UUID name: `pasted-<uuid>.png`** — collision-safe but every
  repeat paste creates a new file, and names carry no dedup value.
- **(F3) Timestamp name: `pasted-2026-08-27-140312.png`** — human-friendly
  but *not* collision-safe (two peers pasting within the same second—or
  with skewed clocks—collide with different content; LWW data loss).

**Recommendation: F1**, with F3's readability recoverable later via rename
(files are ordinary project files; the sidebar rename flow exists).

### D3. Destination directory

- **(P1) Same directory as the current document** — matches the drop
  flow's default (`resolveDefaultDestination({selection})`), yields
  `![](pasted-<hash8>.png)` with no path segments. Simple, consistent.
- **(P2) An `images/` subdirectory next to the document** — keeps the
  sidebar tidy when pasting is frequent, at the cost of inventing a
  convention (and creating the folder implicitly).

**Recommendation: P1 for v1** (consistency with drop; no new convention);
P2 could later become a project setting if paste-heavy projects ask for it.

### D4. Insertion behavior

- Insert `buildDropMarkdown('image', currentFile.path, result.path)` at
  the **current cursor position**, via `executeEdits` (same as drop; the
  CRDT propagation is the already-proven synchronous `onChange` → splice
  path).
- If the user has a **non-empty selection**, replace it and reuse the
  selected text as alt text: `![selected text](href)` — a cheap,
  discoverable nicety.
- **Multiple files in one paste**: insert one reference per file,
  **space-separated** (decided 2026-08-27), in `clipboardData.files`
  order. Newlines are ruled out because the insertion point may be inside
  an indentation-sensitive context (blockquote, indented bullet list),
  where a reference starting at column 0 would have to rely on
  soft-break/lazy-continuation behavior to stay attached to the
  surrounding block; a single-line insertion is safe anywhere the cursor
  can legally sit.
- **No placeholder/spinner needed**: `createBinaryFile` is local CRDT work
  plus a SHA-256 — milliseconds at the 10 MB cap; sync to peers happens in
  the background. Insert after the create resolves so we can use the
  *returned* path (which is authoritative under rename-on-conflict).
- Cursor guard: if the paste races a file switch (`currentFile` changed
  between event and create-resolve), drop the insertion rather than
  inserting into the wrong document (same discipline
  `pendingDropPositionRef` follows).

### D5. Validation and failure UX

- Enforce `FILE_SIZE_LIMITS.MAX_FILE_SIZE` (10 MB) via
  `validateFileSize`; on violation show the same error surface the asset
  dialog uses (or a toast) and insert nothing.
- Empty/undecodable files (size 0): do not preventDefault, so the paste
  degrades to Monaco's own handling — which per §2d means filename-text
  insertion, not a no-op.
- Non-image files in the payload (per §D1 we only take over when *all*
  files are accepted images): fall through to Monaco. An alternative —
  routing non-image file pastes to the asset dialog like drops do — is
  deliberately **out of scope** for v1 (paste-of-copied-PDF is rare; the
  fall-through behavior is today's behavior).

### D6. What we are *not* building (scope fence)

- No async-clipboard "Paste image" toolbar button (§2b) — extension point
  only.
- No paste support in the rich-text/comment surfaces — source editor only.
- No SVG sanitization pipeline (per §3c recommendation S1) — separate
  strand for the pipeline-wide SVG posture.
- No hub-server `Content-Security-Policy` work — separate strand if/when
  raw-file serving exists.

## 5. Work items (TDD order)

- [x] File follow-up strands and link `related` to bd-706b0ixu:
      bd-yspyic32 (mixed payloads), bd-myoj9kp5 (pipeline-wide SVG
      posture).
- [ ] **Tests first** — a pure, DOM-free decision module
      `fileUpload/pasteImages.ts`:
      `classifyPastePayload({files: {name, type}[], text})` →
      `'take-over' | 'pass-through'`, and
      `pastedImageFilename(hash, mimeType)` → `pasted-<hash8>.<ext>`.
      Unit-test the §2a precedence matrix (screenshot, Chrome copy-image,
      Finder file copy incl. filename-text rider, Excel/Office mixed
      payload, SVG file, empty file list, multi-file, unsupported raster)
      and the filename/extension map. Verify tests fail before
      implementing.
- [ ] Extend `buildDropMarkdown` with optional alt text (test-first).
- [ ] Testable ingest orchestration: `createPasteImageHandler(deps)` in
      `fileUpload/` — a factory taking injected deps (classify, process,
      createBinaryFile, markdown builder, editor accessors) so the full
      paste flow (size validation, sequential ingest, single-file
      selection-as-alt, multi-file space join, file-switch guard) is unit
      tested without jsdom or Monaco. Verify tests fail, then implement.
- [ ] Wire a stable capture-phase `paste` listener in `Editor.tsx`
      (attached once at editor mount, delegating through a ref to avoid
      the stale-closure trap), routing through the tested module.
- [ ] `npm run build:all` + `npm run test:ci` (hub-client gates), plus
      workspace Rust gates untouched-but-verified per pre-push checklist.
- [ ] **End-to-end verification** per CLAUDE.md: real browser session
      against a running hub; paste a real image from the OS clipboard;
      observe file creation, markdown insertion, and preview rendering;
      record the invocation + observed output in this plan.
- [ ] hub-client changelog entry (two-commit workflow).
- [ ] Close bd-706b0ixu with pointers.

## Open questions

All resolved 2026-08-27 — see the decision log at the top of this
document.

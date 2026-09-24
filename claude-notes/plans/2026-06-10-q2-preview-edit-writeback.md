# q2 preview: persist React-preview edits back to the source .qmd on disk

**Strand:** bd-ov4gqk3m
**Parent epic:** bd-kw93 (q2 preview epic)
**Status:** design agreed (2026-06-10) — flag is `--allow-edit`; absent ⇒ fully
read-only preview; ≤5 s latency accepted given SIGINT flush (verified to exist)

## Overview

PR #260 (merge `32f1ef59`) added inline block editing to q2-preview documents:
clicking a `Para`/`Header` activates `contentEditable`, and a committed edit is
spliced back into the source QMD via the WASM entry point `apply_node_edit`. In
**hub-client**, the resulting QMD is written into the Automerge file document,
so the edit persists and syncs to all peers.

In **`q2 preview`** (the CLI), the same edit currently stops at the in-browser
WASM VFS. The SPA's edit handler carries an explicit deferral comment
(`q2-preview-spa/src/PreviewApp.tsx:388-391`):

> does NOT yet write back to the Automerge document — a sync event will
> overwrite the local edit on the next change. Full Automerge write-back is a
> Phase 5 follow-up.

(See `claude-notes/plans/2026-06-04-target-incremental-writes.md`, Phase 4.)

**Goal:** edits made in the React preview served by `q2 preview` should end up
in the `.qmd` file on disk that the preview was started from — gated behind a
CLI flag so users don't change their documents by accident.

## Key insight from code study

**The server side already has automerge→disk write-back.** `q2 preview` embeds
the full quarto-hub server, which maintains one Automerge document per file
(`ROOT.text`) and syncs it with disk using a *bidirectional* fork-and-merge
algorithm:

- `crates/quarto-hub/src/sync.rs:49` — `sync_document()` forks the doc at the
  last sync checkpoint, applies current disk content to the fork, merges the
  fork back, and **writes the merged content to disk if it differs** (step 6,
  `sync.rs:148-162`). SHA-256 content-hash checkpoints
  (`sync_state.rs`) prevent echo loops with the file watcher.
- `crates/quarto-hub/src/server.rs:1304` — `run_periodic_sync()` calls
  `sync_all()` on an interval; **preview already configures this to 5 s**
  (`crates/quarto-preview/src/lib.rs:298`, `sync_interval_secs: Some(5)`).
  A final `sync_all()` also runs on shutdown (`server.rs:1293`).
- The samod Rust API has no per-document remote-change observer, which is
  *why* the periodic sync exists — and it is sufficient for us.

**Therefore the missing link is almost entirely client-side:** the SPA never
writes its edit into the Automerge document. The moment it does, the existing
server machinery persists it to disk within ≤5 s (and on Ctrl-C). The
client-side API for this already exists and is already linked into the SPA's
runtime:

- `ts-packages/preview-runtime/src/automergeSync.ts:215` —
  `applyEditorOperations(path, changes)` (splices `EditorContentChange[]` into
  the doc's `text`; delegates to
  `ts-packages/quarto-sync-client/src/client.ts:642`).
- `ts-packages/preview-runtime/src/automergeSync.ts:178` —
  `getFileContent(path)` (current Automerge-side content, the correct diff
  base).

hub-client's precedent for exactly this operation is
`handleContentRewrite` (`hub-client/src/hooks/useAutomergeSync.ts:240-249`):
`diffToEditorChanges(oldContent, newQmd)` → `applyEditorOperations`.

## Current data flows (for reference)

### hub-client (works today)

```
ReactPreview.handleSetAst (ReactPreview.tsx:445)
  → applyNodeEdit (WASM apply_node_edit, crates/pampa/src/apply_node_edit.rs)
  → onContentRewrite(newQmd)
  → handleContentRewrite (useAutomergeSync.ts:240)
      diffToEditorChanges(getFileContent(path), newQmd)
  → applyEditorOperations(path, changes)        # Automerge write
  → samod sync → server → (hub persists via its own sync)
```

### q2 preview (today: ephemeral)

```
PreviewApp.handleSetAst (PreviewApp.tsx:392)
  → applyNodeEdit (same WASM)
  → vfsAddFile(activeFile, newQmd)              # local WASM VFS only
  → contentTick++                               # re-render
  ✗ never reaches Automerge ⇒ never reaches the server ⇒ never reaches disk
  ✗ next sync event from the server overwrites the edit
```

### q2 preview server (machinery already in place)

```
disk change → FileWatcher (watch.rs, 500ms debounce) → ctx.sync_file
browser edit → /ws samod → server doc updated
every 5s + shutdown → run_periodic_sync → sync_all → sync_document
  → fork-at-checkpoint, apply disk text, merge, write merged text to disk
  → SHA-256 checkpoint prevents watcher echo
```

## Design

### 1. CLI flag: `--allow-edit` (default: absent ⇒ read-only)

Add `--allow-edit` to `q2 preview` (`crates/quarto/src/commands/preview.rs`):

- **Absent (default):** the preview is **fully read-only**. No edit
  affordance (no hover outline, no pointer cursor, no contentEditable
  activation), no Automerge writes from the SPA, and no automerge→disk
  writes server-side (see §5). The previous "ephemeral VFS-only editing"
  middle state goes away in the CLI preview. hub-client is unaffected
  (it stays editable as today).
- **Present:** edit surface enabled; SPA routes committed edits into the
  Automerge doc; server persists to disk via the existing sync machinery.

*(Decided 2026-06-10: name `--allow-edit`; absent disables editing entirely.)*

### 2. Plumb the flag to the SPA

The SPA must know whether to enable editing. Add a tiny preview-specific
config endpoint in `extend_with_preview`
(`crates/quarto-preview/src/lib.rs:315`):

```
GET /api/preview/config  →  { "allowEdit": true|false }
```

(Chosen over extending `/health`, which lives in hub-generic
`quarto-hub/src/server.rs:457` and shouldn't grow preview-only fields.)
The SPA fetches it once at boot alongside `/health`.

### 3. Read-only mode in the preview renderer (low effort — verified)

There is currently no global editing toggle: `PreviewContext` always provides
`commitTextEdit`/`commitSubtreeEdit`/`setEditTarget`, and per-block
editability is decided locally by the `isEditable` guard in `Para.tsx:11-13` /
`Header.tsx:14-16` (`ts-packages/preview-renderer/src/q2-preview/blocks/`).
The hover/keyboard/touch affordance lives in
`ts-packages/preview-renderer/src/q2-preview/useBlockEditHover.tsx`.

The clean, purely-additive change is a boolean threaded through the existing
prop/postMessage chain:

1. `Q2PreviewIframe` (`ts-packages/preview-renderer/src/iframe/Q2PreviewIframe.tsx`)
   gains an `editingDisabled?: boolean` prop, forwarded in the `UPDATE_AST`
   postMessage payload (\~line 220).
2. `entry.tsx`\'s `PreviewRoot`
   (`ts-packages/preview-renderer/src/q2-preview/entry.tsx:372-585`) unpacks
   it and exposes `editingDisabled` on `PreviewContextValue`
   (`PreviewContext.tsx:26-50`, default `false` for back-compat).
3. `Para.tsx` / `Header.tsx` add `&& !ctx?.editingDisabled` to `isEditable`
   (this also drops the `data-block-pool-id` attribute, which the affordance
   CSS keys on).
4. `useBlockEditHover.tsx` early-returns in `activate()` and skips the
   outline/stylesheet when disabled, so the preview is completely inert.
5. `PreviewApp.tsx` passes `editingDisabled={!allowEdit}` (from
   `/api/preview/config`); hub-client's `ReactRenderer` passes nothing and
   keeps its current always-editable behavior.

### 4. SPA edit path (the core write-back change)

In `PreviewApp.handleSetAst` (`q2-preview-spa/src/PreviewApp.tsx:392`), after
`applyNodeEdit` succeeds:

```ts
if (!allowEdit) return; // defense in depth; affordance is already disabled
const oldContent = getFileContent(state.activeFile) ?? '';
const changes = diffToEditorChanges(oldContent, newQmd);
if (changes.length > 0) applyEditorOperations(state.activeFile, changes);
// keep existing optimistic local update:
vfsAddFile(state.activeFile, newQmd);
setState(contentTick + 1);
```

Notes:

- The diff base must be `getFileContent` (Automerge-side content), not the VFS
  content — same reasoning as hub-client's `handleContentRewrite`.
- The Automerge change echoes back through the existing `onFileContent`
  handler, which re-runs `vfsAddFile` + `contentTick` with identical content —
  idempotent, same path remote changes already take. The optimistic local
  update just makes the re-render immediate.
- `diffToEditorChanges` currently lives in
  `hub-client/src/utils/diffToMonacoEdits.ts:122`. It is pure (depends only on
  `fast-diff` + the `EditorContentChange` type, which is *already* defined in
  `@quarto/preview-runtime`). **Move it to `ts-packages/preview-runtime`** and
  have hub-client import it from there (the Monaco-coupled
  `diffToMonacoEdits` stays in hub-client). Its tests move with it.

### 5. Server-side enforcement (defense in depth)

With `--allow-edit` absent, the SPA won't write to Automerge — but `/ws` accepts any
loopback client, and the periodic sync would happily persist whatever lands in
a doc. The flag should be trustworthy at the server, not just in our client:

- Add `disk_write_back: bool` to `HubConfig`
  (`crates/quarto-hub/src/context.rs`). Default `true` (hub server semantics
  unchanged); `quarto-preview` sets it from the CLI flag
  (`build_hub_config`, `crates/quarto-preview/src/lib.rs:275`).
- When `false`, `sync_document` (`crates/quarto-hub/src/sync.rs`) skips the
  step-6 disk write and records the **filesystem** content hash in the
  checkpoint, keeping disk authoritative. The fork-and-merge still merges disk
  changes into the doc (so the preview keeps live-updating from disk edits);
  browser-originated doc changes simply never reach disk.
- Checkpoint semantics need a focused unit test: after a doc-only change with
  `disk_write_back=false`, (a) disk content unchanged, (b) a subsequent
  disk-side edit still syncs disk→doc cleanly, (c) no sync-loop churn.

### 6. Write latency and SIGINT flush

v1 relies on the existing 5 s periodic sync plus the shutdown `sync_all`. An
edit therefore reaches disk within ≤5 s. If that feels sluggish in practice, a
follow-up can add `POST /api/preview/sync-file {path}` that the SPA calls
after committing an edit (note: a samod propagation race means the POST may
arrive before the server has received the WS change — harmless, since the
periodic sync catches anything the eager trigger misses).

**SIGINT flush — already implemented.** `run_server_with`
(`crates/quarto-hub/src/server.rs:1097`, body at 1209-1293) listens for
SIGTERM/SIGINT/Ctrl-C, gracefully shuts down the axum server, stops the
periodic-sync and watcher tasks, and then runs a **final `sync_all()`**
(`server.rs:1290-1293`) before returning — so outstanding browser edits the
server has received are flushed to disk on Ctrl-C. The e2e verification phase
must exercise this explicitly (edit → immediate Ctrl-C → file contains the
edit). Accepted residual race: an edit committed milliseconds before SIGINT
may not yet have crossed the WebSocket; that one edit is lost (best-effort).

### 7. Concurrent edits / conflict semantics

If the user edits the file in their text editor while also editing in the
preview, the existing CRDT fork-and-merge merges both — this is precisely the
hub's designed behavior, and we inherit it for free. `apply_node_edit` itself
operates on the render-time content snapshot (`state.renderedContent`), same
as hub-client; the diff-and-splice then applies against current doc content.
Same races, same semantics as hub-client today.

## Resolved questions (2026-06-10, with Carlos)

1. **Flag name:** `--allow-edit`.
2. **Flag-absent UX:** preview editing is **disallowed entirely** (no
   affordance) — confirmed low-effort via the `editingDisabled` context flag
   (§3). The ephemeral-editing middle state is dropped for the CLI preview.
3. **Latency:** ≤5 s periodic sync is acceptable, **conditional on**
   outstanding changes being flushed on SIGINT — verified already implemented
   (§6) and to be exercised explicitly in e2e.

## Remaining open questions

1. **Single-file mode** (bd-tnm3k): no special-casing expected — the watched
   file has a doc like any other — but verify during e2e.

## Work items

### Phase 1 — Tests first (TDD)

- [x] Rust unit tests in `quarto-hub` for ReadOnly sync semantics
      (doc-only change → no disk write; disk change still syncs disk→doc;
      checkpoint stays stable across repeated syncs; disk edit converges doc
      to disk; binary doc-only change reverts to fs content). 5 tests in
      `sync.rs::tests`, written first and verified red (file was written
      despite ReadOnly), then green after gating.
- [x] Rust test for `GET /api/preview/config` reflecting the flag
      (`quarto-preview`): `tests/integration/config_endpoint.rs` boots a real
      server each way and asserts both the JSON wire shape **and** the disk
      behavior (doc-side edit + `sync_file` reaches disk only with
      `allow_edit`). Verified red (SPA fallback served HTML) before the
      endpoint existed.
- [x] TS unit tests for `diffToEditorChanges` in
      `ts-packages/preview-runtime` (7 tests — exact splice offsets plus
      apply-the-ops round-trip property, incl. unicode/emoji). Note: the
      hub-client test file only covered `diffToMonacoEdits`; these are new
      tests, not relocated ones.
- [x] TS test for the SPA edit path
      (`q2-preview-spa/src/channelRouting.integration.test.tsx`, 5 new cases,
      verified red first): with allowEdit on, `handleSetAst` calls
      `applyEditorOperations(activeFile, diffToEditorChanges(getFileContent,
      newQmd))` plus the optimistic `vfsAddFile`; with allowEdit off the
      payload is dropped entirely; a missing config endpoint fails closed;
      `editingDisabled={!allowEdit}` reaches the iframe both ways.
- [x] TS test for read-only mode: with `editingDisabled` set, `Para`/`Header`
      render without `data-block-pool-id` and `useBlockEditHover` does not
      activate (no `setEditTarget` call, no hover outline, no affordance
      stylesheet) — 7 new cases across `q2-preview.integration.test.tsx` and
      `useBlockEditHover.integration.test.tsx`, verified red first.

### Phase 2 — Implementation

- [x] Move `diffToEditorChanges` to `ts-packages/preview-runtime`
      (`src/diffToEditorChanges.ts`, exported from index; `fast-diff` added to
      package deps); hub-client now imports it from `@quarto/preview-runtime`
      (`useAutomergeSync.ts` + test mocks updated; `diffToMonacoEdits` stays
      hub-client-local).
- [x] `--allow-edit` flag in `crates/quarto/src/main.rs` (clap) →
      `PreviewArgs` → `PreviewConfig.allow_edit` → `build_hub_config`
      (`disk_write_policy = WriteBack` iff allow_edit). Verified in
      `q2 preview --help`.
- [x] `HubConfig.disk_write_policy` (`DiskWritePolicy::{WriteBack, ReadOnly}`
      enum — richer than the planned bool) + gating in `sync_document` and
      `sync_binary_document`; threaded through `sync_document_auto`,
      `sync_file_by_path`, `sync_all_documents`, and `HubContext` (initial,
      periodic, and watcher syncs). `quarto-preview` sets `ReadOnly`
      unconditionally for now (flag wiring is the next item); `hub` binary and
      `quarto hub` stay `WriteBack`. ReadOnly semantics: disk authoritative —
      text docs keep doc-side changes merged until the next disk edit
      converges the doc back to disk content; binary docs revert immediately.
- [x] `GET /api/preview/config` endpoint in `extend_with_preview`, returning
      `{ "allowEdit": bool }` from an `ALLOW_EDIT` OnceLock (same pattern as
      the other preview handler state).
- [x] Read-only plumbing in `preview-renderer`: `editingDisabled` through
      `Q2PreviewIframe` props → `UPDATE_AST` payload → `PreviewRoot` →
      `PreviewContext`; guards in `Para.tsx`, `Header.tsx`,
      `useBlockEditHover.tsx` (§3). Hosts that pass nothing (hub-client)
      keep today's always-editable behavior.
- [x] SPA: `fetchAllowEdit()` at boot (fail-closed); `allowEdit` in
      `PreviewAppState`; `editingDisabled={!allowEdit}` on the iframe;
      `handleSetAst` routes the new QMD into Automerge via
      `diffToEditorChanges` + `applyEditorOperations` (diff base =
      `getFileContent`, hub-client's `handleContentRewrite` pattern), keeps
      the optimistic VFS update, and early-returns when read-only.

### Phase 3 — Verification (all done 2026-06-10; output inspected)

- [x] `cargo xtask verify` (full, all 12 steps): **"✓ All verification steps
      passed!"** (includes `npm run build:all` + hub-client `test:ci`).
- [x] End-to-end positive case. Chain rebuilt
      (`cargo xtask build-q2-preview-spa` → `cargo build --bin q2`), then:
      ```
      cargo run --bin q2 -- preview tmp-e2e-allow-edit/doc.qmd --allow-edit --no-browser --port 7341
      ```
      `GET /api/preview/config` → `{"allowEdit": true}`. Clicked the paragraph
      in Chrome (real PointerEvents), edited the textarea, committed via blur;
      the file on disk read back as:
      ```
      ---
      title: Edit write-back e2e
      ---

      This paragraph was EDITED IN THE BROWSER and must reach disk.

      A second paragraph that must stay untouched.
      ```
      Frontmatter and the sibling paragraph were untouched. Output inspected
      directly (`cat`).
- [x] E2e negative case. Same fixture, **without** `--allow-edit`:
      `GET /api/preview/config` → `{"allowEdit": false}`; in the browser,
      zero `[data-block-pool-id]` elements, no affordance stylesheet, cursor
      stays `auto`, and clicking the paragraph produces no edit surface. After
      several sync cycles + SIGINT shutdown, the file's SHA-1
      (`68bf80bd8be8…`) was byte-identical to the pre-session content.
- [x] E2e SIGINT flush. With `RUST_LOG=quarto_hub=debug`: commit edit →
      `kill -INT` ~6 s later → log shows `Performing final filesystem sync
      before shutdown…` / `Final filesystem sync complete synced=1 errors=0`
      and the file contains the edit. **Measured residual race:** in one run
      the SIGINT landed ~1.5 s after commit and the edit was lost — the WS
      frame had not yet reached the server when shutdown began, so there was
      nothing to flush. This is the accepted best-effort window from §6, but
      it is wider than "milliseconds"; the eager-sync follow-up strand should
      close it (SPA could also flush on `visibilitychange`).
- [x] Automated e2e harness: not added in this strand — filed as a follow-up
      strand (Playwright spec driving the real `q2 preview` binary).

### Phase 4 — Bookkeeping

- [x] hub-client changelog: deliberately **no entry**. The only
      hub-client-touching commit (`0072c42f`) is a pure refactor (import of
      `diffToEditorChanges` repointed to `@quarto/preview-runtime`; no
      behavior change), and the changelog header excludes refactors.
- [x] Follow-ups filed as discovered-from strands:
      - bd-g4uw7d8g — eager sync trigger (`POST /api/preview/sync-file` +
        SPA flush on visibilitychange) to close the ≤5 s latency and the
        measured ~1.5 s SIGINT race.
      - bd-f8d753iq — automated Playwright e2e for the `--allow-edit`
        round-trip against the real binary.
- [x] Merge topic branch + close strand (Carlos tested and approved,
      2026-06-10).

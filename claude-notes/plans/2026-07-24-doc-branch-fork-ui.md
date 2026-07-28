# Per-document fork/branch UI in hub-client

## Overview

Experiment: a "fork" button for individual documents in hub-client projects.
Each text document has an implicit **main** branch (the synced automerge doc
that already exists). The user can fork the current state into a local-only
branch, switch between branches in a bar above the editor, edit a branch in
Monaco, and later "Merge to main" — a true CRDT merge (`A.merge`) back into
the synced doc, so concurrent main edits interleave cleanly.

**Key constraint:** forked docs must NOT sync to the sync server. They live in
`localStorage` only. We guarantee this by never registering branch docs with
any `Repo` at all — branches are plain `@automerge/automerge` docs
(`A.clone(handle.doc())`), serialized with `A.save` → base64 → localStorage.
No repo, no network adapter, no sync. Merge-to-main goes through the existing
file `DocHandle.update(d => A.merge(d, branchDoc))`, which fires the sync
client's change path (VFS + Monaco + peers) like any other edit.

## Design

- **`src/services/branchService.ts`** — module singleton (established
  pattern). State keyed by the file's automerge `documentId` (stable across
  renames), API keyed by path (UI currency). localStorage layout:
  - `qh-doc-branches:<docId>` → `{ branches: [{id, name, createdAt}] }`
  - `qh-branch-doc:<docId>:<branchId>` → base64 of `A.save(doc)`
  - Active branch selection is in-memory only (per session; reload → main).
  - Edits: Monaco `event.changes` → `A.change` + `A.splice(d, ['text'], …)`
    (mirrors sync-client `applyEditorOperations`). Persisted to localStorage
    on every edit (no debouncing — removed at elliot's request; `A.save` of
    a qmd-sized doc per keystroke is cheap enough for a prototype).
  - Fork forks the *currently viewed* state (main or another branch).
  - Merge = `handle.update(d => A.merge(d, branchDoc))`, then delete branch,
    switch back to main.
- **`src/hooks/useDocBranches.ts`** — subscribe hook binding a path to the
  service.
- **`useAutomergeSync`** — new `activeBranchId` option; when non-null:
  ignore main-doc immediate sync for that file, reconcile Monaco from
  `branchService.getBranchText`, route `handleEditorChange` to
  `branchService.applyBranchEdits` instead of `onContentOperations`.
  `content` state follows the branch, so the preview pane previews the branch.
- **`src/components/BranchBar.tsx`** (+ CSS) — bar inside
  `div.pane.editor-pane` above the Monaco wrapper (Editor.tsx ~1073),
  modeled on PreviewStatusBar. Chips: `main` + branches; active highlighted;
  inline-input fork creation; per-branch delete; "Merge to main" when on a
  branch. Hidden for binary files; disabled during replay mode.
- **Editor.tsx** — Monaco `key` gains the active branch id (remount on
  switch); presence disabled while on a branch
  (`usePresence(path, { enabled })`).

## Work items

### Phase 1 — tests first (TDD)
- [x] branchService unit tests (jsdom): fork copies text; edits apply to
      branch only; persistence roundtrip across service reset; CRDT merge
      combines concurrent main+branch edits; delete; binary/no-handle guards
- [x] BranchBar component tests: renders chips, switch/fork/merge/delete
      callbacks

### Phase 2 — implementation
- [x] branchService
- [x] useDocBranches
- [x] useAutomergeSync branch gating
- [x] BranchBar + CSS
- [x] Editor.tsx wiring (bar, key, presence enabled flag, replay interplay)

### Phase 3 — verification
- [x] `npx vitest run` new tests fail before impl / pass after
- [x] full hub-client test suite (`npm run test:ci`)
- [x] `npm run build:all`
- [x] Browser end-to-end check — verified manually by elliot (2026-07-24):
      fork → edit → switch → merge works in the running app.
- [ ] Playwright spec `e2e/branch-bar.spec.ts` exists but has NOT passed yet:
      the run reused a stale `vite preview` on port 5174 serving a pre-E2E
      bundle (no `window.__quartoTestReady`), so it failed in bootstrap, not
      in the feature. Kill the stale server (or run a fresh
      `npm run test:e2e`) to exercise it.

## Notes / accepted limitations (experiment scope)

- Branch selection doesn't survive reload (branches themselves do).
- Branches are per-browser (localStorage), invisible to collaborators — by
  design.
- Replay mode and branches are mutually exclusive (bar disabled in replay).
- localStorage quota (~5MB) bounds branch count/size; fine for qmd text.
- Merging does not rebase the branch; it deletes it after merge (git-PR-like
  mental model).

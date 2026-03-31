# Fix: Synchronous Remote Change Application to Prevent Position Mismatch

## Overview

**Bug**: When two users type concurrently, remote Automerge changes are applied to Monaco asynchronously (via React state → useEffect), creating a window where Monaco and Automerge have different text. During this window, local keystrokes produce positions relative to Monaco's stale text, which are applied via `splice()` to Automerge's already-updated text — causing characters to land at wrong positions (letters reversed).

**Root cause**: The sync path from Automerge to Monaco is:
```
handle.on('change') → onFileChanged → setFileContents (React batched) → useEffect → diffToMonacoEdits → executeEdits
```
Between `setFileContents` and the useEffect firing, user keystrokes can happen. Their positions are computed from Monaco (stale) but applied to Automerge (already merged with remote change).

**Fix**: Add a synchronous callback slot in `automergeSync.ts` that fires from within the `onFileChanged` handler, BEFORE the React state update. Editor.tsx registers a callback that applies remote diffs to Monaco **synchronously** — before any subsequent keystroke can fire on the main thread. Local changes are skipped via a `model.getValue() === content` identity check (Monaco already has the correct content for those).

## Bug Analysis

### Scenario 1: Same Paragraph

Document: `"hello"`

1. **User A** types `a` at pos 5 → Monaco: `"helloa"`, Automerge: `"helloa"` ✓
2. **User B** inserts `x` at pos 0 → Automerge: `"xhelloa"`, **Monaco still `"helloa"`** (useEffect pending)
3. **User A** types `b` at pos 6 → Monaco: `"helloab"`, but `splice(automerge, 6, 0, 'b')` applied to `"xhelloa"` → `"xhelloba"` — **b lands before a!**
4. useEffect fires → forces Monaco to `"xhelloba"` — letters reversed

### Scenario 2: Different Paragraphs

The same mechanism causes out-of-order characters even when concurrent typing is in **completely separate paragraphs**, as long as one user's edit is at a lower offset.

Document: `"aaa\n\nbbb"` (paragraph 1 = `aaa`, paragraph 2 = `bbb`)

1. **User A** types `x` at end of paragraph 2 (pos 8) → Monaco: `"aaa\n\nbbbx"`, Automerge: `"aaa\n\nbbbx"` ✓
2. **User B** types `y` at end of paragraph 1 (pos 3) → Automerge: `"aaay\n\nbbbx"` — everything after pos 3 shifts right by 1. **Monaco still `"aaa\n\nbbbx"`** (useEffect pending).
3. **User A** types `z` at pos 9 (after `x` in Monaco's view) → `splice(automerge, 9, 0, 'z')` applied to `"aaay\n\nbbbx"` → inserts at pos 9: `"aaay\n\nbbzx"` — **z lands before x**, not after it.

User A intended `"bbbxz"` in their paragraph, but got `"bbzx"` — their own letters reversed, despite editing a completely different paragraph from User B.

### General Principle

**Any remote insertion at offset R shifts all higher offsets by the length of the inserted text.** It doesn't matter whether the edits are in the same word, same paragraph, or entirely different sections. If:

- Remote edit inserts N characters at offset R
- Local user's next keystroke targets offset L where L > R
- Monaco hasn't been updated yet

Then the splice hits offset L in Automerge's text, but the correct target (from the user's perspective) is at offset L+N. The splice lands N characters too early.

The fix ensures Monaco is always in sync with Automerge before the next keystroke fires, so offset L in Monaco always equals offset L in Automerge.

## Work Items

### Phase 1: Infrastructure

- [x] Add `setImmediateFileChangeCallback(cb | null)` to `automergeSync.ts` — a synchronous callback slot that fires from within the Automerge `onFileChanged` handler, BEFORE the React state update. This avoids Editor.tsx needing to import Automerge types or manage handle subscriptions directly.

### Phase 2: Synchronous Monaco Sync in Editor.tsx

- [x] Register a synchronous file-change callback from the sync effect in `Editor.tsx` that:
  1. Checks the path matches `currentFile`
  2. Skips if `replayState.isActive`
  3. Gets Monaco model content via `model.getValue()`
  4. If Monaco content ≠ incoming content, computes `diffToMonacoEdits` and applies via `executeEdits` with the `applyingRemoteRef` guard
  5. Also calls `setContent()` to keep React state (preview) in sync
- [x] Keep the existing `useEffect` at line 444 as a **fallback** for initial file load, file switching, and any edge cases. It becomes a no-op during active editing because the synchronous callback keeps Monaco in sync.

### Phase 3: Tests

Tests go in `hub-client/src/services/automergeSync.test.ts`, in a new `describe('immediate file change callback')` block. Use the existing `mockSyncClient` infrastructure (`_simulateRemoteChange`, `_setClientForTesting`, `_resetForTesting`). Import `setImmediateFileChangeCallback` from `./automergeSync`.

#### 3a. Service-layer unit tests (automergeSync.test.ts)

- [x] **Callback fires synchronously on remote change**: Register a callback via `setImmediateFileChangeCallback`. Call `mockClient._simulateRemoteChange('test.qmd', 'new content')`. Assert the callback was invoked with `('test.qmd', 'new content')` — critically, assert this **inside the same synchronous flow**, not after `await` or `setTimeout`. Verify by checking that a flag set inside the callback is already `true` immediately after `_simulateRemoteChange` returns.

- [x] **Callback fires synchronously on local splice**: Call `applyEditorOperations('test.qmd', [{ rangeOffset: 5, rangeLength: 0, text: 'X' }])`. Assert the callback fires with the updated content. This verifies the local-change fast-path setup (Editor.tsx will compare `model.getValue() === content` to skip).

- [x] **Callback fires before onFileContent handler**: Register an immediate callback via `setImmediateFileChangeCallback` and an `onFileContent` handler via `setSyncHandlers`. Inside the immediate callback, set a flag. Inside `onFileContent`, assert the flag is already set. This verifies the ordering guarantee: immediate callback runs before the React state update path.

- [x] **Callback receives correct path filtering data**: Simulate changes to two different files (`a.qmd` and `b.qmd`). Assert the callback receives the correct `(path, content)` pair for each — the callback itself doesn't filter, but it receives the path so Editor.tsx can.

- [x] **Null callback is safe**: Set callback to `null` via `setImmediateFileChangeCallback(null)`. Call `_simulateRemoteChange`. Assert no error is thrown (the `?.()` optional call works).

- [x] **Callback replacement**: Register callback A, then replace with callback B. Simulate a change. Assert only B was called, not A. This verifies cleanup on file switch (useEffect cleanup sets null, then re-registers).

#### 3b. Position-correctness scenario test (automergeSync.test.ts)

- [x] **Race condition scenario — same paragraph**: Reproduce the exact bug from the plan's Scenario 1. Setup: file with `"hello"`. Steps:
  1. Local user types `a` at pos 5: `applyEditorOperations('test.qmd', [{ rangeOffset: 5, rangeLength: 0, text: 'a' }])` → content becomes `"helloa"`
  2. Remote user inserts `x` at pos 0: `mockClient._simulateRemoteChange('test.qmd', 'xhelloa')` — capture the content delivered to the immediate callback
  3. Assert callback received `"xhelloa"` (the immediate callback would sync Monaco here)
  4. Local user types `b` at pos 7 **in the now-synced Monaco** (offset 7 in `"xhelloa"` = after `a` at index 6): `applyEditorOperations('test.qmd', [{ rangeOffset: 7, rangeLength: 0, text: 'b' }])` — offset 7, not 6, because Monaco is now synced to `"xhelloa"` and the user's cursor shifted right by 1
  5. Assert `getFileContent('test.qmd') === 'xhelloab'` — letters in correct order

- [x] **Race condition scenario — cross-paragraph**: Reproduce Scenario 2. Setup: file with `"aaa\n\nbbb"`. Steps:
  1. Local user types `x` at pos 8: `applyEditorOperations` → `"aaa\n\nbbbx"`
  2. Remote user types `y` at pos 3: `_simulateRemoteChange('test.qmd', 'aaay\n\nbbbx')`
  3. Assert callback received `"aaay\n\nbbbx"`
  4. Local user types `z` at pos 10 (after `x` in synced content `"aaay\n\nbbbx"`): `applyEditorOperations` with offset 10
  5. Assert `getFileContent('test.qmd') === 'aaay\n\nbbbxz'` — `z` after `x`, not before

- [x] **Without the fix (regression guard)**: Document the **incorrect** behavior that would occur without the synchronous callback. In a comment, show that if step 3 used the stale Monaco offset (6 instead of 7 in Scenario 1), `splice(6, 0, 'b')` on `"xhelloa"` would produce `"xhelloba"` — letters reversed. The test's correct offset (7) represents the fixed behavior where Monaco was synced before the keystroke.

### Phase 4: Verification

- [x] Run `cargo xtask verify --skip-rust-tests` to ensure hub-client builds and passes tests (Rust build ✓, hub-client build ✓, 396 unit/integration tests pass ✓, 4 pre-existing WASM test failures in formatDetection unrelated to this change)
- [ ] Manual testing scenario: open same doc in two browser tabs, type rapidly in both, verify no letter reversals

## Design Details

### Why not subscribe directly to the DocHandle from Editor.tsx?

Option considered: call `getFileHandle(path)` and `handle.on('change', ...)` from Editor.tsx, using `patchInfo.source` to distinguish local vs remote.

Rejected because:
1. Requires importing Automerge types (`DocHandleChangePayload`, `PatchInfo`) into Editor.tsx
2. Handle lifecycle management (subscribe/unsubscribe on file switch) duplicates what `client.ts` already does
3. If the handle is replaced (reconnect), Editor.tsx wouldn't know

### Chosen approach: Module-level immediate callback in `automergeSync.ts`

Add a synchronous callback slot that fires from within the existing `onFileChanged` chain:

```typescript
// automergeSync.ts
type ImmediateFileChangeCallback = (path: string, content: string) => void;
let immediateFileChangeCallback: ImmediateFileChangeCallback | null = null;

export function setImmediateFileChangeCallback(cb: ImmediateFileChangeCallback | null) {
  immediateFileChangeCallback = cb;
}

// In ensureClient(), inside the callbacks object passed to SyncClient:
// The onFileChanged callback (automergeSync.ts ~line 80) becomes:
onFileChanged: (path: string, text: string, patches: Patch[]) => {
  vfsAddFile(path, text);
  immediateFileChangeCallback?.(path, text);  // ← synchronous, BEFORE React state update
  onFileContent?.(path, text, patches);       // ← triggers React state update (setSyncHandlers handler)
},
```

In Editor.tsx:
```typescript
useEffect(() => {
  if (!currentFile) return;

  const handleImmediateSync = (path: string, content: string) => {
    if (path !== currentFile.path) return;
    if (replayActiveRef.current) return;

    const editor = editorRef.current;
    const model = editor?.getModel();
    if (!editor || !model) return;

    const monacoContent = model.getValue();
    if (monacoContent === content) return;  // Local change → already in sync

    const edits = diffToMonacoEdits(monacoContent, content);
    if (edits.length > 0) {
      applyingRemoteRef.current = true;
      editor.executeEdits('remote-sync', edits);
      applyingRemoteRef.current = false;
    }
    setContent(content);
  };

  setImmediateFileChangeCallback(handleImmediateSync);
  return () => setImmediateFileChangeCallback(null);
}, [currentFile, replayState.isActive]);
```

### Why this works

1. **Local changes**: User types → `handleEditorChange` → `applyEditorOperations` → `handle.change(splice)` → Automerge fires change event → `onFileChanged` → `immediateFileChangeCallback`. Monaco already has the content → `monacoContent === content` → **no-op** (fast: one `getValue()` + one string comparison).

2. **Remote changes**: WebSocket message → Automerge merge → change event → `onFileChanged` → `immediateFileChangeCallback`. Monaco is stale → `monacoContent !== content` → compute diff → `executeEdits`. This happens **synchronously within the WebSocket message handler**, so no user keystroke can interleave. The `applyingRemoteRef` guard prevents echo.

3. **Existing useEffect becomes a safety net**: After the synchronous callback, React state is also updated via `setFileContents`. The useEffect fires, reads Automerge, reads Monaco (already synced) → no-op. It still handles edge cases like initial load (before callback is registered).

### Performance

- Local changes: O(n) for `model.getValue()` + O(1) for string identity check. Monaco caches `getValue()`, so practical cost is minimal.
- Remote changes: Same cost as current useEffect approach (diff + executeEdits), just synchronous instead of deferred. No regression.
- The `fast-diff` library's fast path (`currentContent === targetContent`) returns `[]` immediately, so the overhead for local changes is negligible.

## Review Findings

### Relationship to commit 40faca40

This plan addresses a **different bug** from the one fixed in 40faca40. Two distinct races exist:

- **Bug A (fixed by 40faca40):** The sync useEffect read `fileContents` from its closure (render-time) but `model.getValue()` from the DOM (effect-time), causing the effect to incorrectly revert user edits when the stale closure didn't reflect the latest typing. Fixed by reading live Automerge content via `getFileContent()`.

- **Bug B (this plan):** Remote Automerge changes update the CRDT text immediately, but Monaco remains stale until the passive useEffect fires (after paint). During this window, user keystrokes generate positions relative to Monaco's stale text, but `applyEditorOperations` applies `splice()` to Automerge's already-updated text — positions are offset, causing characters to land at wrong locations.

The two fixes are complementary, not conflicting. 40faca40 prevents the sync effect from reverting correct edits; this plan prevents wrong-position splices during the async gap.

### Bug diagnosis: Correct

The scenario analysis is valid. The general principle — any remote insertion at offset R shifts all higher offsets — applies to any concurrent editing scenario regardless of paragraph boundaries. The cross-paragraph example (Scenario 2) is a strong illustration that overlapping edit regions are not required.

### Proposed fix: Correct

The synchronous callback approach works because JavaScript is single-threaded:

1. WebSocket `onmessage` fires as a macrotask → Automerge merges → `onFileChanged` → synchronous callback → Monaco updated via `executeEdits` → macrotask returns
2. Queued keystroke fires as next macrotask → Monaco and Automerge are in sync → positions correct

No user input can interleave between the Automerge merge and the Monaco update because they share the same synchronous call stack.

- **Local change fast path:** Correct. After a local splice, `model.getValue()` already reflects the keystroke, so `monacoContent === content` → no-op.
- **Echo prevention:** Correct. The `applyingRemoteRef` guard prevents `executeEdits` from triggering `handleEditorChange` → infinite loop.
- **Existing useEffect as fallback:** Correct. Handles initial load, file switching, and edge cases where the callback isn't registered.

### Edge cases reviewed

1. **Callback registration gap on file switch.** When `currentFile` changes, cleanup runs before re-registration. A remote change in this micro-window gets no callback. **Acceptable** — the useEffect fallback handles it, and users aren't typing at the exact instant of a file switch.

2. **`model.getValue()` cost.** O(n) on every Automerge change (local and remote), but Monaco caches it. Negligible for typical document sizes. For very large documents (100K+ lines), the string comparison could be a bottleneck, but this is a pre-existing concern shared with the current useEffect approach.

3. **Module-level singleton.** Only one consumer supported. Fine for the current single-Editor architecture; would need refactoring for multiple editors. **Acceptable** — no hypothetical future requirements.

4. **`setContent()` inside the synchronous callback.** Triggers a React state update from a WebSocket handler. In React 18+, this is batched and deferred — desired behavior for preview state. **No issue.**

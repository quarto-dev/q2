# Operation-Based Sync: Fix Concurrent Edit Race Condition

**Issue:** quarto-dev/q2#74 — hub: edits can get lost
**Related PR:** #80 (partial fix, merged 2026-03-25)

## Overview

The hub-client uses `updateText()` to sync editor content to Automerge. `updateText` computes a full-text diff between the current Automerge document state and the content string passed to it. When remote edits merge into the Automerge document between the time Monaco's content is read and the time `updateText` executes, the diff deletes the remote content — because Monaco doesn't have it yet.

**Fix:** Replace full-text `updateText()` with positional `splice()` operations derived from Monaco's `IModelContentChange` events. Each keystroke/edit produces exact `{rangeOffset, rangeLength, text}` triples that map directly to Automerge `splice(doc, ['text'], offset, deleteCount, insertText)`. Since Automerge's CRDT handles concurrent positional splices natively, no full-text diff is needed and the race window is eliminated.

## Architecture

### Current flow (broken under concurrency)

```
User types → Monaco onChange(value, event)
  → handleEditorChange(value)
    → onContentChange(path, value)         // full content string
      → updateFileContent(path, value)     // in client.ts
        → handle.change(doc => updateText(doc, ['text'], value))
                                           // ↑ full-text diff against
                                           //   current Automerge state
```

### New flow (operation-based)

```
Any local edit (typing, drag-drop, paste, find-replace, undo/redo)
  → Monaco onChange(value, event)
    → handleEditorChange(value, event)
      → setContent(value)                              // React state for preview
      → onContentOperations(path, event.changes)       // NEW: positional ops
        → applyEditorOperations(path, changes)         // in client.ts
          → handle.change(doc => {
              for (change of changes) {
                splice(doc, ['text'], change.rangeOffset,
                       change.rangeLength, change.text)
              }
            })

ReactPreview AST rewrite (e.g., slide reorder)
  → setContent(newQmd)                                 // prop = handleContentRewrite
    → editorRef.executeEdits('ast-rewrite', fullRangeEdit)
      → Monaco onChange(value, event)                  // fires synchronously
        → (same splice path as above)
```

All local edits — typed, pasted, drag-dropped, find-replaced, or AST-rewritten — flow through the same `onChange` → splice path. The explicit `onContentChange` callback and `updateFileContent` full-text path are removed from Editor entirely. `updateFileContent` remains only for replay mode (`useReplayMode.ts:159`), which operates outside the Editor component.

### Key design decisions

1. **Single unified path for all local edits** — Every local edit (including drag-drop `executeEdits`) triggers Monaco's `onChange`, which fires `handleEditorChange`. This calls `setContent(value)` for React state and `onContentOperations(path, event.changes)` for the CRDT. No edit needs a separate sync call.

2. **Remove `onContentChange` prop from Editor** — The old `onContentChange` callback carried full content strings for `updateText`. With all local edits routed through splice, this prop is no longer needed. Drag-drop code (lines 765, 861) no longer explicitly calls `onContentChange` after `executeEdits` — Monaco's `onChange` handles it.

3. **Remove `handleContentChange` from App.tsx** — With `onContentChange` gone, `handleContentChange` (which called `updateFileContent`) is removed. `handleContentOperations` (which calls `applyEditorOperations`) replaces it.

4. **Route ReactPreview AST writes through Monaco** — `ReactPreview.tsx:212` calls `setContent(newQmd)` after AST modifications via `incrementalWriteQmd`. Rather than calling `updateFileContent` (which has the same `updateText` race condition), we route through Monaco: `handleContentRewrite` applies the new content to the Monaco editor via `executeEdits('ast-rewrite', ...)`, which fires `onChange`, which flows through the splice path. This:
   - **Eliminates the race**: Monaco's model is always up-to-date with remote edits, so the splice is diffed against the correct base state.
   - **Eliminates a wiring gap**: No need to pipe `updateFileContent` into Editor — `handleContentRewrite` only needs `editorRef`, which Editor already has.
   - **Preserves undo**: `executeEdits` pushes onto Monaco's undo stack, so Ctrl+Z reverses an AST rewrite.
   - **Unifies all mutation paths**: Every content change flows through Monaco → `onChange` → splice.

5. **Keep `updateFileContent` only for replay mode** — `useReplayMode.ts:159` calls `updateFileContent` directly to set replay snapshots. This operates outside the Editor component and doesn't go through Monaco's `onChange`. It is the sole remaining consumer of `updateFileContent` from the Editor's perspective.

6. **Changes are applied in a single `handle.change()` call** — Monaco batches changes (e.g., multi-cursor, find-replace). All changes from one event go into one Automerge transaction.

7. **Changes are ordered end-to-beginning** — Monaco documents: "The changes are ordered from the end of the document to the beginning, so they should be safe to apply in sequence." For `splice`, we need to apply them in this same order (end-first) so earlier offsets remain valid.

## Work Items

### Phase 0: UTF-16 encoding alignment (DONE)

Both the JS/WASM client and the Rust server must agree on text index encoding. Monaco and the Automerge WASM build both use UTF-16 code units (JavaScript's native string encoding). The Rust server previously defaulted to `UnicodeCodePoint`, which disagrees for non-BMP characters (emoji etc.) — each emoji is 1 code point but 2 UTF-16 code units.

- [x] **0.1** Enable `utf16-indexing` feature on `automerge` dependency in `quarto-hub/Cargo.toml`
- [x] **0.2** Add `test_text_encoding_is_utf16` — assert `TextEncoding::platform_default() == Utf16CodeUnit`
- [x] **0.3** Add `test_splice_text_with_emoji` — verify `splice_text` offsets are correct after a non-BMP character (🎉 = 2 UTF-16 code units)
- [x] **0.4** Add `test_splice_text_delete_emoji` — verify deleting an emoji requires `deleteCount=2`
- [x] **0.5** Add `test_splice_text_replace_after_multiple_emoji` — verify offsets accumulate correctly with 3 consecutive emoji
- [x] **0.6** Add `test_update_text_preserves_emoji_in_concurrent_edits` — concurrent splices around emoji survive merge
- [x] **0.7** Add `test_splice_text_with_mixed_bmp_and_non_bmp` — mix of CJK (1 code unit) and emoji (2 code units)
- [x] **0.8** Verify all 172 existing `quarto-hub` tests still pass with the new feature flag

### Phase 1: Tests (TDD — write tests before implementation)

- [x] **1.1** Add `EditorContentChange` type to `types.ts` (mirrors Monaco's `IModelContentChange` shape: `{ rangeOffset: number; rangeLength: number; text: string }`)
- [x] **1.2** Write unit test for `applyEditorOperations` in client — single insert, single delete, replace, multi-change batch, multi-cursor (N changes at arbitrary positions verifying end-to-beginning ordering) (test will not compile yet — that's expected)
- [ ] **1.3** Write integration test simulating concurrent edits: local splice + remote merge should not lose remote content (deferred: requires real Automerge; covered by Rust-side tests in Phase 0)
- [x] **1.4** Write test with non-BMP characters (emoji) in `applyEditorOperations` — verify UTF-16 offsets from Monaco map correctly through Automerge `splice`
- [x] **1.5** Write test that `applyEditorOperations` with an empty changes array is a no-op (no Automerge transaction created)

### Phase 2: Add `splice` import and new client API

- [x] **2.1** Add `splice` import to `client.ts` (from `@automerge/automerge-repo` — verified exported)
- [x] **2.2** Add `applyEditorOperations(path, changes)` function to `client.ts` that applies positional splice operations
- [x] **2.3** Export `applyEditorOperations` from the sync client's public API (add to the returned object from `createSyncClient`)
- [x] **2.4** Export `applyEditorOperations` from `automergeSync.ts` service layer
- [x] **2.5** Run Phase 1 tests — verify they pass (all 9 tests pass)

### Phase 3: Wire Editor to use operations and remove old path

- [x] **3.1** Add `onContentOperations` prop to Editor component (`(path: string, changes: EditorContentChange[]) => void`)
- [x] **3.2** Update `handleEditorChange` in Editor.tsx to accept the `IModelContentChangedEvent` (second arg from Monaco's `onChange`) and call `onContentOperations` with `event.changes`
- [x] **3.3** Remove `onContentChange` prop from Editor component entirely
- [x] **3.4** Remove the post-`executeEdits` sync blocks from drag-drop code (lines ~760–766 and ~857–862): remove the `getValue()`, `setContent()`, and `onContentChange()` calls. Also remove `onContentChange` from the `useCallback` dependency arrays at lines ~796 and ~869. Monaco's `onChange` now handles both React state and CRDT sync via the splice path.
- [x] **3.5** Route ReactPreview AST writes through Monaco: introduce `handleContentRewrite` in Editor that applies new content via `diffToMonacoEdits` → `executeEdits('ast-rewrite', ...)`. This fires `onChange`, which flows through the splice path — no direct access to `updateFileContent` needed. Pass `handleContentRewrite` as the `onContentRewrite` prop to `PreviewRouter` instead of passing `handleEditorChange` as `setContent`.
- [x] **3.6** Add `handleContentOperations` callback in App.tsx that calls `applyEditorOperations`
- [x] **3.7** Remove `handleContentChange` and `onContentChange` prop from App.tsx — no longer needed
- [x] **3.8** Pass `onContentOperations={handleContentOperations}` to Editor
- [x] **3.9** Verify replay mode (`useReplayMode.ts:159`) still works — it calls `updateFileContent` directly, which remains available (sole remaining consumer outside Editor)
- [ ] **3.10** Verify ReactPreview AST writes (`ReactPreview.tsx:212`) still work — `onContentRewrite` now routes through `handleContentRewrite` → `executeEdits` → `onChange` → splice. Test that undo (Ctrl+Z) reverses an AST rewrite. (manual testing required)

### Phase 4: Verification

- [x] **4.1** `cargo build --workspace` passes
- [x] **4.2** `cargo nextest run --workspace` passes (6893 passed)
- [x] **4.3** `cd hub-client && npm run build` passes
- [x] **4.4** `cd hub-client && npm run test:ci` — unit tests (389 passed), integration (12 passed), WASM (48 passed, 4 failed pre-existing in formatDetection.wasm.test.ts — unrelated)
- [ ] **4.5** Manual testing: open two browser tabs, type rapidly in both, verify no content loss
- [ ] **4.6** Manual testing: drag-drop a file into the editor, verify content is inserted correctly
- [ ] **4.7** Manual testing: ReactPreview AST rewrite (e.g., drag-reorder slides) while another tab is typing — verify no content loss
- [ ] **4.8** Manual testing: after a ReactPreview AST rewrite, Ctrl+Z undoes the change

## File Change Summary

| File | Change |
|------|--------|
| `crates/quarto-hub/Cargo.toml` | Enable `utf16-indexing` feature on automerge dependency **(DONE)** |
| `crates/quarto-hub/src/automerge_api_tests.rs` | Add UTF-16 encoding verification and splice tests with emoji **(DONE)** |
| `ts-packages/quarto-sync-client/src/types.ts` | Add `EditorContentChange` type |
| `ts-packages/quarto-sync-client/src/client.ts` | Add `splice` import, add `applyEditorOperations` function, export it from return object |
| `ts-packages/quarto-sync-client/src/index.ts` | Export `EditorContentChange` type and `applyEditorOperations` |
| `hub-client/src/services/automergeSync.ts` | Add `applyEditorOperations` wrapper, export `EditorContentChange` type |
| `hub-client/src/components/Editor.tsx` | Replace `onContentChange` prop with `onContentOperations`, import `EditorContentChange` type, update `handleEditorChange` to accept `IModelContentChangedEvent` second arg, remove post-`executeEdits` sync blocks from drag-drop, add `handleContentRewrite` that routes through `executeEdits` → splice for PreviewRouter's `onContentRewrite` |
| `hub-client/src/App.tsx` | Add `handleContentOperations` callback (imports `applyEditorOperations`), remove `handleContentChange` and `updateFileContent` import, replace `onContentChange` prop with `onContentOperations` |
| `hub-client/src/test-utils/mockSyncClient.ts` | Add `applyEditorOperations` mock |
| `hub-client/src/services/automergeSync.test.ts` | Add tests for `applyEditorOperations` wrapper (if applicable) |

## Detailed Implementation Notes

### `applyEditorOperations` in client.ts

```typescript
function applyEditorOperations(path: string, changes: EditorContentChange[]): void {
  if (changes.length === 0) return;  // no-op guard (edge case 9)

  const handle = state.fileHandles.get(path);
  if (!handle) {
    console.warn(`No handle found for file: ${path}`);
    return;
  }

  handle.change(doc => {
    // Monaco orders changes end-to-beginning, so earlier offsets stay valid.
    for (const change of changes) {
      splice(doc, ['text'], change.rangeOffset, change.rangeLength, change.text);
    }
  });
}
```

### Monaco `onChange` signature

The `@monaco-editor/react` `onChange` prop already provides the event:

```typescript
type OnChange = (value: string | undefined, ev: editor.IModelContentChangedEvent) => void;
```

So `handleEditorChange` just needs to accept the second argument:

```typescript
const handleEditorChange = (value: string | undefined, event: Monaco.editor.IModelContentChangedEvent) => {
  if (replayActiveRef.current) return;
  if (applyingRemoteRef.current) return;

  if (value !== undefined && currentFile) {
    setContent(value);  // React state for preview
    onContentOperations(currentFile.path, event.changes);  // CRDT operations
  }
};
```

### Why drag-drop needs no special handling

Drag-drop calls `executeEdits()`, which modifies the Monaco model synchronously. This fires Monaco's `onChange` synchronously during `executeEdits`, which triggers `handleEditorChange` → `setContent` + `onContentOperations`. By the time `executeEdits` returns, both React state and the CRDT are already updated. The old explicit post-`executeEdits` sync code (`getValue()` → `setContent()` → `onContentChange()`) is therefore redundant and removed.

### Why `updateFileContent` still exists

Only one consumer remains after the refactor:

**Replay mode** (`useReplayMode.ts:159`) calls `updateFileContent` directly to set file content from replay snapshots. This operates outside the Editor component and doesn't go through Monaco's `onChange`. It is the sole remaining consumer.

ReactPreview AST writes no longer use `updateFileContent` — they route through Monaco (see below).

### `handleContentRewrite` in Editor.tsx

ReactPreview AST writes (e.g., drag-reordering slides) produce a new QMD string via `incrementalWriteQmd`. Rather than calling `updateFileContent` directly (which has the same `updateText` race condition as the original bug), the new `handleContentRewrite` routes through Monaco:

```typescript
// Route AST rewrites through Monaco → onChange → splice path.
// Uses diffToMonacoEdits (already in the codebase) to compute minimal edits,
// so concurrent remote edits in unchanged regions merge cleanly via CRDT.
// Also preserves undo history (executeEdits pushes onto the undo stack).
const handleContentRewrite = useCallback((newContent: string) => {
  if (!editorRef.current || !currentFile) return;
  const model = editorRef.current.getModel();
  if (!model) return;

  const oldContent = model.getValue();
  const edits = diffToMonacoEdits(oldContent, newContent);
  if (edits.length > 0) {
    editorRef.current.executeEdits('ast-rewrite', edits);
  }
  // onChange fires synchronously → handleEditorChange → setContent + onContentOperations
  // Each edit becomes a targeted splice, not a full-document replacement.
}, [currentFile]);
```

This replaces the previous `setContent={handleEditorChange}` prop on `PreviewRouter` (line 1086) with `onContentRewrite={handleContentRewrite}`. Rename the prop from `setContent` to `onContentRewrite` in the `PreviewRouter` → `ReactPreview` chain to avoid confusion with the React state setter `setContent` used in `handleEditorChange`. The signature remains `(content: string) => void`.

**Why this is better than `updateFileContent`:**

1. **No race condition** — Monaco's model is always synchronized with remote edits (applied via `executeEdits('remote-sync', ...)`). The `onChange` event diffs against this up-to-date state, not against potentially-stale Automerge document state.
2. **No wiring gap** — `handleContentRewrite` only needs `editorRef` (already available in Editor), not `updateFileContent` (which lives in `client.ts` and would require a new prop).
3. **Undo support** — `executeEdits` pushes onto Monaco's undo stack. Users can Ctrl+Z to reverse an AST rewrite (e.g., undo a slide reorder).
4. **Single mutation path** — All content changes (typing, drag-drop, AST rewrites) flow through Monaco → `onChange` → splice.
5. **Fine-grained CRDT merges** — `diffToMonacoEdits` (already used in Editor.tsx for remote sync) computes minimal edits via `fast-diff`. Monaco's `onChange` then produces targeted changes that become targeted Automerge splices. Concurrent remote edits in unchanged regions merge cleanly, unlike a full-document `splice(0, fullLen, newContent)` which would displace them.

### Edge cases

1. **Undo/redo**: Monaco fires `onChange` for undo/redo with the same `IModelContentChange` format. These map naturally to splice operations. No special handling needed.

2. **Find-replace**: Monaco batches all replacements into a single `onChange` event with multiple changes. The batch goes into one `handle.change()` transaction. The end-to-beginning ordering ensures correctness.

3. **Paste**: Large pastes are a single change with `rangeLength` (selection replaced) and `text` (pasted content). Maps directly to a single splice.

4. **Remote edits arriving during local edit**: The Automerge CRDT handles concurrent splice operations correctly — that's its core design. Two users inserting at the same position will both see both insertions (in a deterministic order). No content is lost.

5. **`applyingRemoteRef` guard**: When remote edits are applied via `executeEdits('remote-sync', ...)`, Monaco fires `onChange`. The `applyingRemoteRef` check in `handleEditorChange` prevents these from being sent back to Automerge as local operations. This guard already exists and works correctly.

6. **Drag-drop via `executeEdits`**: `executeEdits` modifies the Monaco model synchronously, which fires `onChange` synchronously. The splice path in `handleEditorChange` handles both React state and CRDT sync. No explicit post-`executeEdits` sync is needed. This unifies all local edits through one code path.

7. **ReactPreview AST writes**: `ReactPreview` calls `setContent(newQmd)` after converting a modified AST back to QMD text. This routes through `handleContentRewrite`, which uses `diffToMonacoEdits` (already in the codebase) to compute minimal edits, then applies them via `executeEdits('ast-rewrite', ...)` → Monaco `onChange` → splice path. Because the edits are fine-grained (not a full-document replacement), concurrent remote edits in unchanged regions merge cleanly at the CRDT level. The `applyingRemoteRef` guard is `false` during AST writes (they are local edits), so the splice operations are correctly sent to Automerge.

8. **Non-BMP characters (emoji, supplementary CJK, etc.)**: Monaco's `rangeOffset`/`rangeLength` count UTF-16 code units (JavaScript's native string encoding). Automerge's `splice` must use the same encoding. The WASM build of Automerge enables `utf16-indexing` by default. The Rust server now also enables `utf16-indexing` via Cargo feature flag (Phase 0). Both sides agree: emoji like 🎉 (U+1F389) count as 2 units, BMP characters count as 1. This is verified by tests in `automerge_api_tests.rs`.

9. **Empty changes array**: Monaco could theoretically fire `onChange` with an empty changes array. Guard with `if (changes.length === 0) return;` in `applyEditorOperations` to avoid a no-op Automerge transaction.

## Risk Assessment

**Low risk:** The core change is small — we're replacing one Automerge API (`updateText`, full-text diff) with another (`splice`, positional). Both are first-class Automerge APIs. The rest is plumbing.

**Simplification benefit:** By routing all local edits (including drag-drop) through the splice path, we *reduce* the API surface — `onContentChange` prop and `handleContentChange` are removed entirely. Fewer code paths means fewer places for bugs.

**Regression risk:** Replay mode continues using `updateFileContent`/`updateText`, which is low risk (it operates outside the Editor and isn't a concurrent-typing scenario). ReactPreview AST writes now route through Monaco → `diffToMonacoEdits` → fine-grained `executeEdits` → splice, eliminating the residual `updateText` race, gaining undo support, and preserving fine-grained CRDT merges for concurrent edits.

**Correctness depends on:** Monaco's documented guarantee that `changes` are ordered end-to-beginning and that `rangeOffset`/`rangeLength` are relative to the document state *before* any changes in the batch are applied. This is well-documented behavior.

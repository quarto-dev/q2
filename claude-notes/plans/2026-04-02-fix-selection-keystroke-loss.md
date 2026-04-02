# Fix: First keystroke lost after selection in Monaco editor

## Overview

Two separate bugs caused the first keystroke to be lost after making a selection in the hub-client Monaco editor:

1. **Intermittent (any selection direction)**: The `handleEditorChange` callback was recreated on every render, causing `@monaco-editor/react` to re-subscribe its `onDidChangeModelContent` listener via `useEffect` on every render. Keystrokes landing between paint and effect execution could be dropped.

2. **Deterministic (backward/RTL selections only)**: On some platforms, the browser's hidden textarea input pipeline silently drops the first character typed into a backward selection. Monaco receives the `keyDown` event but the `input` event never fires on the hidden textarea, so no model change occurs.

## Bug 1: Unstable onChange callback

### Root Cause

`@monaco-editor/react` v4.7.0 manages its `onDidChangeModelContent` listener inside a `useEffect([isReady, onChange])`. Because `handleEditorChange` was not memoized, it received a new function reference on every React render, causing the library to dispose and re-subscribe its listener on every render. Keystrokes landing in the window between paint and effect execution could be silently dropped.

A secondary issue: the `options` object passed to `<MonacoEditor>` was defined inline in JSX, creating a new reference every render, causing unnecessary `editor.updateOptions()` calls.

### Fix

- **`useAutomergeSync.ts`**: Added `currentFileRef` (a `useRef` that mirrors `currentFile`). Wrapped `handleEditorChange` in `useCallback([onContentOperations])` — stable across all renders since `onContentOperations` is itself `useCallback([], [])`.
- **`Editor.tsx`**: Promoted the inline `options` to a module-level `const editorOptions` (fully static, no `useMemo` needed).

### Tests

Two regression tests in `useAutomergeSync.test.ts`:
1. **Stable identity across re-renders** — asserts `handleEditorChange` is `===` across re-renders with different `fileContents`.
2. **Ref picks up file switches** — asserts the stable callback routes changes to the new file path after a file switch.

## Bug 2: Backward selection drops first keystroke

### Root Cause

On some platforms (confirmed on macOS), when the user makes a backward (RTL) selection in Monaco and types a character, the browser's input pipeline silently drops the keystroke. Diagnosis via instrumentation confirmed:
- `editor.onKeyDown` fires (`code=KeyS, hasSelection=true, selDir=RTL`)
- `model.onDidChangeContent` does NOT fire
- The `@monaco-editor/react` `onChange` callback is never called

The issue is at the browser/OS level: Monaco's hidden textarea has its selection set to represent the backward selection, and the platform's input method system fails to process the first keystroke in this state. This was confirmed by ruling out all application-level causes (selection sync, presence, reconciliation) via targeted disabling.

### Fix

**`Editor.tsx`**: Added an `editor.onKeyDown` handler in `handleEditorMount` that normalizes backward selections to forward on any printable keyDown. When a printable character key is pressed with an RTL selection active, the handler calls `editor.setSelection()` to flip the selection to LTR (same highlighted range, cursor moves to end). This allows the browser's input pipeline to process the character correctly.

```typescript
editor.onKeyDown((e) => {
  const sel = editor.getSelection();
  if (!sel || sel.isEmpty() || sel.getDirection() === 0) return;
  const key = e.browserEvent.key;
  if (!key || key.length !== 1) return;
  editor.setSelection({
    selectionStartLineNumber: sel.startLineNumber,
    selectionStartColumn: sel.startColumn,
    positionLineNumber: sel.endLineNumber,
    positionColumn: sel.endColumn,
  });
});
```

## Analysis

### What was ruled out

- **Automerge sync path**: Fully synchronous. `getFileContent()` always matches `model.getValue()` for local edits.
- **Reconciliation effect**: Always a no-op for local edits (reads live Automerge state, not stale closure).
- **Selection sync (`useSelectionSync`)**: Disabled entirely during debugging — backward selection bug persisted.
- **Presence hook**: `onDidChangeCursorSelection` handler only sends presence data, doesn't modify editor.
- **`@monaco-editor/react` controlled mode**: We use `defaultValue` (uncontrolled), library's value-sync code is inert.
- **Preview iframe stealing focus**: `editor.focus()` after `preview.setSelection()` didn't fix the issue.
- **MorphIframe `selectionchange` feedback loop**: Suppressing programmatic `selectionchange` events didn't fix the issue.

### Diagnostic approach for Bug 2

Instrumentation was added at three levels:
1. **Sync layer** (`handleEditorChange`): logged all calls including dropped ones (replay/remote guards). Result: no log at all for backward selections → callback never invoked.
2. **Monaco events** (`editor.onKeyDown`, `model.onDidChangeContent`): `keyDown` fired but `modelChange` did not → keystroke received by Monaco but never applied to model.
3. **Selection sync disabled**: Bug persisted → not caused by selection sync.

This narrowed the cause to the browser's input pipeline between Monaco's `keyDown` handler and the hidden textarea's `input` event.

### Key files

| File | Role |
|------|------|
| `hub-client/src/hooks/useAutomergeSync.ts` | Bidirectional Automerge ↔ Monaco sync |
| `hub-client/src/hooks/useAutomergeSync.test.ts` | Regression tests for callback stability |
| `hub-client/src/components/Editor.tsx` | Main editor component, RTL selection workaround |

## Changes

- [x] Diagnose and fix intermittent keystroke loss (unstable onChange callback)
- [x] Diagnose and fix deterministic backward selection keystroke loss (RTL normalization)
- [x] Add regression tests for callback stability
- [x] Simplification pass (comment trimming, useMemo → module constant)
- [x] Remove debug instrumentation
- [x] Revert unsuccessful fixes (MorphIframe settingSelectionRef, editor.focus in useSelectionSync)
- [x] Verify TypeScript compiles cleanly
- [x] Verify all 398 hub-client tests pass

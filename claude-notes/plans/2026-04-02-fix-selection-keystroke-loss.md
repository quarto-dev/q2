# Fix: First keystroke lost after selection in Monaco editor

## Overview

After making a selection in the hub-client Monaco editor, the first keystroke was sometimes silently dropped instead of replacing the selection. This was an intermittent race condition between React's asynchronous `useEffect` scheduling and `@monaco-editor/react`'s internal `onDidChangeModelContent` listener lifecycle.

## Root Cause

`@monaco-editor/react` v4.7.0 manages its `onDidChangeModelContent` listener inside a `useEffect([isReady, onChange])`. Because `handleEditorChange` (passed as the `onChange` prop) was **not memoized**, it received a new function reference on every React render. This caused the library to **dispose and re-subscribe** its content-change listener on every render.

The race window:

1. User makes a selection — this triggers state updates (presence `setModelVersion`, etc.) which queue a React re-render.
2. React commits and the browser **paints**.
3. User types to replace the selection (between paint and pending `useEffect` execution).
4. Monaco fires `onDidChangeModelContent`, but the listener is mid-teardown — the `useEffect` from the re-render is about to dispose it and create a new one.
5. The event can be silently dropped when it coincides with the disposal/re-subscription cycle.

The intermittent nature comes from requiring the keystroke to land in the narrow window between paint and effect execution — a window that only opens when a re-render was triggered (e.g., by presence state updates from the selection event itself).

A secondary issue: the `options` object passed to `<MonacoEditor>` was defined inline in JSX, creating a new reference every render. This caused `@monaco-editor/react`'s separate `useEffect([options])` to call `editor.updateOptions()` on every render — unnecessary work that widened the timing window.

## Analysis

### What was ruled out

- **Automerge sync path**: `applyEditorOperations` → `handle.change()` → `onFileChanged` is fully synchronous. `getFileContent()` always returns the current Automerge state, so the reconciliation effect is always a no-op for local edits.
- **Reconciliation effect reverting changes**: Since `getFileContent()` reads directly from the Automerge client (not React state), it always matches `model.getValue()` for local edits.
- **Selection sync**: `useSelectionSync` only syncs editor → preview for non-collapsed selections; the preview never sends selection events back automatically.
- **Presence hook**: `model.onDidChangeContent` fires `setModelVersion` but only triggers decoration updates via `deltaDecorations`, which doesn't affect selection or content.
- **`@monaco-editor/react` controlled mode**: We use `defaultValue` (uncontrolled), so the library's value-sync code (`B.current` / `preventTriggerChangeEvent`) is inert.

### Key files examined

| File | Role |
|------|------|
| `hub-client/src/hooks/useAutomergeSync.ts` | Bidirectional Automerge ↔ Monaco sync |
| `hub-client/src/components/Editor.tsx` | Main editor component, mounts Monaco |
| `hub-client/src/services/automergeSync.ts` | Automerge client wrapper |
| `ts-packages/quarto-sync-client/src/client.ts` | Sync client implementation |
| `hub-client/node_modules/@monaco-editor/react/dist/index.mjs` | Library internals |
| `hub-client/src/hooks/usePresence.ts` | Collaborative cursor OT |
| `hub-client/src/hooks/useSelectionSync.ts` | Editor ↔ preview selection sync |
| `hub-client/src/utils/diffToMonacoEdits.ts` | Diff → Monaco edit operations |

## Changes

- [x] Diagnose root cause of intermittent keystroke loss after selection
- [x] Stabilize `handleEditorChange` with `useCallback` + `currentFileRef`
- [x] Promote static `editorOptions` to module-level constant
- [x] Add regression tests
- [x] Simplification pass (comment trimming, useMemo → module constant)
- [x] Verify TypeScript compiles cleanly
- [x] Verify all 398 hub-client tests pass

### `hub-client/src/hooks/useAutomergeSync.ts`

- Added `currentFileRef` (a `useRef` that mirrors `currentFile`) so the callback can read the latest file without depending on it.
- Wrapped `handleEditorChange` in `useCallback([onContentOperations])`. Since `onContentOperations` is itself `useCallback([], [])` in App.tsx, `handleEditorChange` is now **stable across all renders** (identity only changes on mount).

### `hub-client/src/hooks/useAutomergeSync.test.ts`

Two regression tests added to `describe('handleEditorChange')`.

### Testing strategy

The actual race condition (keystroke landing between browser paint and React `useEffect` execution) cannot be reproduced in a jsdom unit test — it requires real browser event-loop timing. Instead, the tests guard the **invariant that the fix establishes**: `handleEditorChange` must be a referentially stable callback.

1. **Stable identity across re-renders** — renders the hook, then re-renders with different `fileContents` maps (the most frequent trigger for re-renders during normal editing). Asserts `result.current.handleEditorChange` is `===` to the original reference. If someone removes the `useCallback`, adds an unstable dependency, or inlines the function again, this test fails immediately.

2. **Ref picks up file switches** — renders with `file1`, re-renders with `file2`, then calls `handleEditorChange` and asserts the change is routed to `file2.qmd`. This guards against the complementary mistake of over-stabilizing: if someone replaces the `currentFileRef` with a stale closure capture, the test catches it because the callback would still route to `file1.qmd`.

Together the two tests form a pincer: test 1 prevents the callback from becoming unstable, test 2 prevents it from becoming stale.

### `hub-client/src/components/Editor.tsx`

- Promoted the inline `options={{...}}` to a module-level `const editorOptions` — fully static, never references component state, so no `useMemo` needed. Communicates intent more clearly and avoids per-instance hook overhead.
- `as const` annotations on string literal values (`'on'`, `'off'`) are necessary because Monaco's `IStandaloneEditorConstructionOptions` expects string literal unions, not `string`.
- Replaced inline options with `options={editorOptions}` on the `<MonacoEditor>` component.

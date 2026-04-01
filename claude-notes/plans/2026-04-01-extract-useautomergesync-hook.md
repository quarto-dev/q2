# Refactor: Extract `useAutomergeSync` Hook

## Context

The bidirectional Automerge↔Monaco sync logic in `Editor.tsx` spans ~80 lines across 3 effects + 2 handlers, interleaved with unrelated UI concerns. Following the clean separation pattern from automerge-codemirror (`codeMirrorToAm` / `amToCodemirror`), we extract this into a focused `useAutomergeSync` hook. This follows the existing pattern of `usePresence`, `useReplayMode`, etc.

## Hook Interface

```typescript
// Inputs
interface UseAutomergeSyncOptions {
  currentFile: FileEntry | null;
  fileContents: Map<string, string>;
  onContentOperations: (path: string, changes: EditorContentChange[]) => void;
  replayActiveRef: React.RefObject<boolean>;
  replayIsActive: boolean;
}

// Outputs
interface UseAutomergeSyncResult {
  content: string;
  setContent: React.Dispatch<React.SetStateAction<string>>;
  applyingRemoteRef: React.MutableRefObject<boolean>;
  handleEditorChange: (value: string | undefined, event: Monaco.editor.IModelContentChangedEvent) => void;
  handleContentRewrite: (newContent: string) => void;
  onEditorMount: (editor: Monaco.editor.IStandaloneCodeEditor) => void;
}
```

## What Moves Into the Hook

| Code | Editor.tsx lines |
|---|---|
| `applyingRemoteRef` | 145 |
| `content` / `setContent` state | 172 |
| Real-time remote edit sync | 431-460 |
| Reconciliation on mount/file-switch | 462-514 |
| `handleEditorChange` | 551-562 |
| `handleContentRewrite` | 567-578 |

## What Stays in Editor.tsx

- All replay effects (use `applyingRemoteRef` from hook)
- File switching code (calls `setContent` from hook)
- Monaco config, mount, markers, drag/drop
- Presence, intelligence, scroll sync hooks

## Implementation Steps

- [x] Create `hub-client/src/hooks/useAutomergeSync.ts` with the hook
- [x] Wire hook into `Editor.tsx`: remove extracted code, call hook, compose `onEditorMount` in handleEditorDidMount
- [x] Add tests in `hub-client/src/hooks/useAutomergeSync.test.ts`
- [x] Run `npm run test:ci` from hub-client to verify

## Key Design Decisions

1. **Hook owns `content` state** — it's fundamentally a sync product. Exposed via `setContent` for file-switching code.
2. **Hook owns its own `editorRef`** — populated via `onEditorMount` callback, same pattern as `usePresence`.
3. **`applyingRemoteRef` exposed** — replay code needs it when applying replay edits.
4. **Content initialized to `''`** — the reconciliation effect sets it on first render. No flash because Monaco uses `defaultValue` (uncontrolled).
5. **`replayIsActive` as separate boolean** — needed for effect dependency arrays (refs don't trigger re-renders).

## Two Automerge → Monaco Sync Paths

The hook has two distinct paths for pushing Automerge content into Monaco:

1. **Real-time remote edits** — Synchronous callback within the same macrotask as the WebSocket handler. Updates Monaco *before* the user's next keystroke reads positions. Only fires on actual Automerge change events. This is the fix for the position-correctness bug (PR #102).

2. **Reconciliation on mount / file switch** — React effect that fires on initial mount, file switch, or `fileContents` changes. Handles cases where no Automerge change event fires. For ongoing remote edits this is usually a no-op since the real-time callback already handled it.

## Files

- **New**: `hub-client/src/hooks/useAutomergeSync.ts`
- **New**: `hub-client/src/hooks/useAutomergeSync.test.ts`
- **Modified**: `hub-client/src/components/Editor.tsx`
- **Minor comment update**: `hub-client/src/services/automergeSync.ts`
- **Unchanged**: `hub-client/src/utils/diffToMonacoEdits.ts`

## Verification

```bash
cd hub-client && npm run test:ci
cd hub-client && npm run build
```

# Fix Monaco Cursor Shift During Remote Edits

**Beads Issue:** k-rmdm
**Status:** Implemented
**Created:** 2025-12-28
**Implemented:** 2025-12-28

## Problem Summary

In collaborative editing sessions using the quarto-hub frontend, when a remote collaborator makes changes to the document, the local user's cursor position shifts unexpectedly. This significantly degrades the collaborative editing experience.

## Root Cause

The current implementation passes the **entire document content as a string** from Automerge to Monaco on every remote change:

```
Remote edit arrives via Automerge
    ↓
changeHandler() emits full text string (no position info)
    ↓
React state updates with new content
    ↓
Monaco receives new `value` prop
    ↓
Monaco replaces entire document model
    ↓
CURSOR POSITION LOST
```

Monaco has no way to know what changed or where, so it cannot preserve cursor position.

### Affected Code Paths

| File | Issue |
|------|-------|
| `automergeSync.ts:288-293` | `changeHandler` emits full `text` string, discards patch info |
| `App.tsx:27-32` | `onFileContent` receives only content, no change metadata |
| `Editor.tsx:231-238` | Replaces content via `setContent()`, triggering full re-render |
| `Editor.tsx:299-313` | Monaco `value` prop causes document replacement |

## Solution

Use Monaco's [`executeEdits()`](https://microsoft.github.io/monaco-editor/typedoc/interfaces/editor.ICodeEditor.html) API to apply incremental edits instead of replacing the entire document.

**Key insight:** Automerge's change event already provides [patches](https://automerge.org/docs/reference/repositories/dochandles/) describing exactly what changed. We convert these patches to Monaco edit operations and apply them directly. Monaco then handles cursor preservation automatically.

### Data Flow (After Fix)

```
Remote edit arrives via Automerge
    ↓
changeHandler() forwards patches + content
    ↓
Convert Automerge patches → Monaco edit operations
    ↓
editor.executeEdits('remote-sync', edits)
    ↓
Monaco applies edits incrementally
    ↓
CURSOR POSITION PRESERVED
```

## Implementation

### Phase 1: Infrastructure Setup

**1. Add Monaco editor instance ref** (`Editor.tsx`)

```typescript
import type * as Monaco from 'monaco-editor';

const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);

<MonacoEditor
  onMount={(editor) => { editorRef.current = editor; }}
  // ... other props
/>
```

**2. Update type signatures to include patches** (`automergeSync.ts`)

```typescript
// Change handler signature
type FileContentHandler = (path: string, content: string, patches: Patch[]) => void;

// In subscribeToFile, forward patches from change event
const changeHandler = ({ patches }: { patches: Patch[] }) => {
  const changedDoc = handle.doc();
  if (changedDoc) {
    vfsAddFile(path, changedDoc.text || '');
    onFileContent?.(path, changedDoc.text || '', patches);
  }
};
```

### Phase 2: Patch Conversion Utility

**3. Create `hub-client/src/utils/patchToMonacoEdits.ts`**

```typescript
import type { Patch } from '@automerge/automerge';
import type * as Monaco from 'monaco-editor';

/**
 * Convert character offset to Monaco position (1-indexed line/column).
 */
function offsetToPosition(content: string, offset: number): Monaco.IPosition {
  let line = 1;
  let column = 1;
  for (let i = 0; i < offset && i < content.length; i++) {
    if (content[i] === '\n') {
      line++;
      column = 1;
    } else {
      column++;
    }
  }
  return { lineNumber: line, column };
}

/**
 * Convert Automerge patches to Monaco edit operations.
 */
export function patchesToMonacoEdits(
  patches: Patch[],
  currentContent: string
): Monaco.editor.IIdentifiedSingleEditOperation[] {
  const edits: Monaco.editor.IIdentifiedSingleEditOperation[] = [];

  for (const patch of patches) {
    // Only process patches targeting the 'text' field
    if (patch.path[0] !== 'text') continue;

    const { action } = patch;

    if (action.type === 'splice') {
      // Insert: empty range at insert point
      const pos = offsetToPosition(currentContent, action.index);
      edits.push({
        range: {
          startLineNumber: pos.lineNumber,
          startColumn: pos.column,
          endLineNumber: pos.lineNumber,
          endColumn: pos.column,
        },
        text: action.value,
        forceMoveMarkers: true,
      });
    } else if (action.type === 'del') {
      // Delete: range covering deleted text
      const startPos = offsetToPosition(currentContent, action.index);
      const endPos = offsetToPosition(currentContent, action.index + action.length);
      edits.push({
        range: {
          startLineNumber: startPos.lineNumber,
          startColumn: startPos.column,
          endLineNumber: endPos.lineNumber,
          endColumn: endPos.column,
        },
        text: '',
      });
    }
  }

  return edits;
}
```

### Phase 3: Wire Up Edit Application

**4. Update Editor.tsx to apply edits incrementally**

```typescript
// Track whether we're applying remote changes (to prevent echo)
const applyingRemoteRef = useRef(false);

// Handle remote content changes via patches
useEffect(() => {
  if (!currentFile || !editorRef.current) return;

  const patches = filePatches.get(currentFile.path);
  const newContent = fileContents.get(currentFile.path);

  if (patches && patches.length > 0 && newContent !== content) {
    // Incremental update: convert patches to Monaco edits
    const edits = patchesToMonacoEdits(patches, content);

    if (edits.length > 0) {
      applyingRemoteRef.current = true;
      editorRef.current.executeEdits('remote-sync', edits);
      applyingRemoteRef.current = false;
    }

    // Sync local state
    setContent(newContent);
  } else if (newContent !== undefined && newContent !== content) {
    // Fallback: full content replacement (initial load, no patches)
    setContent(newContent);
  }
}, [currentFile, fileContents, filePatches]);

// Prevent local changes from echoing back
const handleEditorChange = (value: string | undefined) => {
  if (applyingRemoteRef.current) return;
  if (value !== undefined && currentFile) {
    setContent(value);
    onContentChange(currentFile.path, value);
  }
};
```

### Phase 4: Edge Cases

**5. Handle edge cases:**

- **No patches available** (initial load) → fall back to full content replacement
- **Empty patch list** → no-op, content already matches
- **Editor not mounted** → skip edit application, React will handle via props
- **Patches for wrong file** → ignore (filtered by `currentFile.path`)

**6. Local vs remote change handling:**

- Local typing: `handleEditorChange` → `onContentChange` → Automerge
- Remote changes: Automerge → patches → `executeEdits` (flag prevents echo)

## Files to Modify

| File | Changes |
|------|---------|
| `hub-client/src/services/automergeSync.ts` | Update `FileContentHandler` type, forward patches from change event |
| `hub-client/src/App.tsx` | Receive patches in `onFileContent`, add `filePatches` state, pass to Editor |
| `hub-client/src/components/Editor.tsx` | Add editor ref + `onMount`, apply edits via `executeEdits()`, echo prevention |
| `hub-client/src/utils/patchToMonacoEdits.ts` | **New file:** patch conversion utility |

## Technical Reference

### Automerge Patch Format

The change event provides patches in this structure ([docs](https://automerge.org/docs/reference/repositories/dochandles/)):

```typescript
{ handle: DocHandle, patches: Patch[], patchInfo: PatchInfo }
```

Text-relevant [PatchAction](https://automerge.org/automerge/automerge/patches/enum.PatchAction.html) types:

| Action | Fields | Description |
|--------|--------|-------------|
| `SpliceText` | `index`, `value` | Insert text at position |
| `DeleteSeq` | `index`, `length` | Delete characters |

Each patch includes a `path` array (e.g., `["text"]`) identifying the affected field.

### Monaco Edit Format

[`IIdentifiedSingleEditOperation`](https://microsoft.github.io/monaco-editor/typedoc/interfaces/editor.IIdentifiedSingleEditOperation.html):

```typescript
{
  range: IRange,              // Region to replace (empty = insert)
  text: string | null,        // New text (empty string = delete)
  forceMoveMarkers?: boolean  // Push markers at insert point
}
```

Monaco coordinates are 1-indexed (line 1, column 1 = start of file).

## Testing

### Unit Tests (`patchToMonacoEdits.ts`)

- `offsetToPosition()`: line breaks, empty content, offset at EOF
- Patch conversion: insert, delete, multi-line, combined operations

### Manual Testing

| Scenario | Expected Result |
|----------|-----------------|
| Remote insert before cursor | Cursor stays at same logical position |
| Remote insert after cursor | Cursor unchanged |
| Remote delete before cursor | Cursor adjusts backward |
| Remote delete spanning cursor | Cursor moves to deletion point |
| Multi-line remote edit | Cursor adjusts correctly |
| Rapid concurrent edits | No cursor jitter |

### Integration Test

1. Open same document in two browser tabs
2. Position cursor in tab A
3. Type in tab B
4. Verify cursor in tab A stays stable

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Automerge patch format differs from expected | Low | Log patches in dev console, adjust parsing |
| Race conditions with rapid edits | Medium | The `applyingRemoteRef` flag prevents echo loops |
| Monaco API changes | Low | Pin `@monaco-editor/react` version |

## Success Criteria

- Cursor stays at logical position when remote edits occur before it
- Cursor remains unchanged when remote edits occur after it
- Cursor moves to deletion point if it was inside deleted text
- Works correctly with multi-line edits
- No cursor flicker during rapid collaborative editing

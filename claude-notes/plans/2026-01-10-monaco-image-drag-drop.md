# Monaco Editor Image Drag-Drop Feature

**Beads Issue:** k-znum
**Created:** 2026-01-10
**Status:** Complete

## Overview

Implement drag-and-drop image upload functionality for the Monaco editor in hub-client. When a user drags an image from their desktop to the editor, the system should:

1. Show the existing upload pane UX (NewFileDialog in upload mode)
2. After successful upload, insert a markdown image snippet `![](filename)` at the drop location

## Research Findings

### Monaco Editor API Capabilities

Monaco provides the necessary APIs for this feature:

| API | Purpose |
|-----|---------|
| `editor.getDomNode()` | Get DOM container for attaching drag-drop listeners |
| `editor.getTargetAtClientPoint(x, y)` | Convert screen coordinates to editor position (line/column) |
| `editor.executeEdits(source, edits)` | Insert text at a specific position |
| `editor.getPosition()` | Get current cursor position (fallback) |

### Current Architecture

**File upload flow (FileSidebar → Editor):**
1. `FileSidebar.tsx:handleDrop` captures dropped files
2. Calls `onUploadFiles(files)` prop
3. `Editor.tsx:handleUploadFiles` stores files and opens dialog
4. `NewFileDialog.tsx` shows upload UI with previews
5. On confirm, calls `onUploadBinaryFile(file)` for each file
6. `Editor.tsx:handleUploadBinaryFile` processes and stores via Automerge

**Binary file creation:**
- `createBinaryFile()` in `automergeSync.ts` returns `{ docId, path, deduplicated }`
- The `path` may differ from original filename due to hash-based deduplication
- This is the path we need for the markdown snippet

### Key Files

| File | Role |
|------|------|
| `hub-client/src/components/Editor.tsx` | Main editor component, coordinates file operations |
| `hub-client/src/components/NewFileDialog.tsx` | Upload dialog with drag-drop zone |
| `hub-client/src/services/automergeSync.ts` | File creation with Automerge |
| `hub-client/src/services/resourceService.ts` | File processing, validation |

## Design

### State Changes in Editor.tsx

New state/refs needed:

```typescript
// Position where image was dropped (for markdown insertion)
const [pendingDropPosition, setPendingDropPosition] = useState<Monaco.IPosition | null>(null);

// Visual feedback for drag-over state
const [isEditorDragOver, setIsEditorDragOver] = useState(false);
```

### Event Handlers

**handleEditorDragOver:**
- Prevent default, set visual feedback
- Only activate for files (check `e.dataTransfer.types.includes('Files')`)

**handleEditorDragLeave:**
- Clear visual feedback

**handleEditorDrop:**
- Prevent default
- Filter for image files only
- Capture drop position using `getTargetAtClientPoint()`
- Store position in state
- Call existing `handleUploadFiles()` with the image files

### Modified Upload Flow

The `NewFileDialog` currently calls `onUploadBinaryFile(file)` which is fire-and-forget. We need to:

1. **Option A:** Modify `handleUploadBinaryFile` to check for pending drop position and insert markdown
2. **Option B:** Add a new callback to `NewFileDialog` that reports uploaded file paths

**Chosen: Option A** - simpler, less prop threading

The flow becomes:
1. User drops image on editor
2. `handleEditorDrop` captures position, stores it, opens dialog
3. User confirms upload in dialog
4. `handleUploadBinaryFile` uploads file, gets actual path
5. If `pendingDropPosition` exists, insert markdown at that position
6. Clear `pendingDropPosition`

### Markdown Insertion

```typescript
const insertMarkdownImage = (path: string, position: Monaco.IPosition) => {
  if (!editorRef.current) return;

  const markdown = `![](${path})`;
  editorRef.current.executeEdits('image-drop', [{
    range: {
      startLineNumber: position.lineNumber,
      startColumn: position.column,
      endLineNumber: position.lineNumber,
      endColumn: position.column,
    },
    text: markdown,
    forceMoveMarkers: true,
  }]);
};
```

### Visual Feedback

Add CSS class `.editor-pane.drag-over` with visual indicator (border glow or overlay).

### Edge Cases

1. **Multiple images:** Insert multiple `![](path)` separated by newlines
2. **Dialog cancelled:** Clear `pendingDropPosition` on dialog close
3. **Position invalid:** If editor content changed significantly, fall back to current cursor position
4. **Non-image files:** Only handle image files for markdown insertion; other files go through normal upload without insertion
5. **Cleanup:** Remove event listeners on unmount

## Implementation Plan

### Phase 1: Core Drag-Drop Infrastructure
- [x] Add drag-drop event listeners to Monaco container in `handleEditorMount`
- [x] Add state for `pendingDropPosition` and `isEditorDragOver`
- [x] Implement `handleEditorDragOver`, `handleEditorDragLeave`, `handleEditorDrop`
- [x] Add cleanup in useEffect

### Phase 2: Modify Upload Flow
- [x] Modify `handleUploadBinaryFile` to use the created path from `createBinaryFile`
- [x] Add markdown insertion logic after successful upload
- [x] Clear `pendingDropPosition` after dialog close via `handleDialogClose`

### Phase 3: Visual Feedback
- [x] Add CSS for drag-over state on editor pane (green border glow + overlay message)
- [x] Show drop zone indicator

### Phase 4: Polish
- [x] Handle image filtering (only images get markdown insertion)
- [x] TypeScript check passes
- [x] Build succeeds

## Files to Modify

1. `hub-client/src/components/Editor.tsx` - Main changes
2. `hub-client/src/components/Editor.css` - Drag-over styling
3. `hub-client/src/services/automergeSync.ts` - May need to ensure async return of path

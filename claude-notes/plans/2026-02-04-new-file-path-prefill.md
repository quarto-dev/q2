# Pre-fill new file dialog with current file's directory path

**Issue:** bd-3cus

## Overview

When the user clicks "+ New" in the file sidebar to create a new file, the filename
input field should be pre-filled with the directory path of the currently active file.
This saves keystrokes when creating files in the same directory as the file being edited.

**Example:** If editing `docs/guides/chapter1.qmd`, the filename field shows `docs/guides/`
and the cursor is placed at the end, so the user only needs to type `chapter2.qmd`.

Files at the root level (e.g., `index.qmd`) should not pre-fill anything.

## Work Items

- [x] Update `handleNewFile` in `Editor.tsx` to extract the directory from `currentFile.path` and set it as `newFileInitialName`
- [x] Verify cursor placement: ensure the cursor lands at the end of the pre-filled path so the user can type immediately
- [x] Test edge cases: root-level files (no directory), deeply nested paths, no file currently open

## Implementation Details

### Files to change

**`hub-client/src/components/Editor.tsx`** (primary change)

The `handleNewFile` callback (line 413) currently does:

```typescript
const handleNewFile = useCallback(() => {
  setPendingUploadFiles([]);
  setShowNewFileDialog(true);
}, []);
```

Change it to extract the directory prefix from `currentFile` and pass it via `setNewFileInitialName`:

```typescript
const handleNewFile = useCallback(() => {
  setPendingUploadFiles([]);
  if (currentFile) {
    const lastSlash = currentFile.path.lastIndexOf('/');
    if (lastSlash >= 0) {
      setNewFileInitialName(currentFile.path.substring(0, lastSlash + 1));
    }
  }
  setShowNewFileDialog(true);
}, [currentFile]);
```

### No changes needed to `NewFileDialog.tsx`

The `initialFilename` prop and its handling already exist and work correctly:
- The `useEffect` at line 108 sets the filename state from `initialFilename`
- The focus effect at line 116 auto-focuses the input
- The reset effect at line 123 clears state when the dialog closes
- The close handler in Editor.tsx (line 745) already clears `newFileInitialName`

### Cursor behavior

When an input is focused and already has a value, the browser places the cursor at the
end of the text by default. This is the desired behavior — the user sees `docs/guides/`
with the cursor at the end and can immediately type the filename.

### Edge cases

- **Root-level file** (`index.qmd`): `lastIndexOf('/')` returns -1, so no pre-fill. Correct.
- **No file open** (`currentFile` is null): The `if (currentFile)` guard prevents errors. Correct.
- **Deeply nested** (`a/b/c/d.qmd`): Pre-fills `a/b/c/`. Correct.

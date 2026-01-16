# Hub-Client Cursor Jump Bug Fix

**Issue:** kyoto-hlr
**Created:** 2026-01-16
**Status:** Fixed

## Problem

During rapid typing in the hub-client editor, the cursor intermittently jumped to the end of the document. This was reproducible in single-user sessions during rapid typing.

## Root Cause

The `@monaco-editor/react` wrapper was configured in "controlled" mode (`value={content}` prop). When React state diverged from Monaco's model content (due to async Automerge updates), the wrapper would detect the mismatch and call `setValue()` to sync them, which reset the cursor position to the end.

## Solution

Switch Monaco to "uncontrolled" mode:

1. **Use `defaultValue` instead of `value`**: The wrapper only uses `defaultValue` for initial content and doesn't try to sync the prop with the model on subsequent renders.

2. **Add `key={currentFile?.path}`**: Forces the editor to remount when switching files, ensuring the new file's content is loaded via `defaultValue`.

3. **Apply remote changes via `executeEdits()`**: Uses diff-based synchronization to apply minimal edits that preserve cursor position. React's `setContent()` still runs to keep preview in sync, but since Monaco is uncontrolled, it doesn't affect the editor.

## Files Changed

- `hub-client/src/components/Editor.tsx` - Switched to uncontrolled mode, simplified sync effect
- `hub-client/src/utils/diffToMonacoEdits.ts` - Utility for diff-based synchronization (created during investigation)
- `hub-client/src/utils/diffToMonacoEdits.test.ts` - Tests for diff utility
- `hub-client/src/utils/patchToMonacoEdits.ts` - Removed (orphaned from previous patch-based approach)

## Key Code Changes

```typescript
// Before (controlled mode - caused cursor jumps)
<MonacoEditor
  value={content}
  onChange={handleEditorChange}
  ...
/>

// After (uncontrolled mode - cursor preserved)
<MonacoEditor
  key={currentFile?.path ?? ''}
  defaultValue={content}
  onChange={handleEditorChange}
  ...
/>
```

## Testing

1. Run `npm run dev` in hub-client
2. Type rapidly in the editor
3. Cursor should stay in place (no jumping to end of file)

---

## Investigation History

This section documents the debugging process for future reference if similar issues arise.

### Initial Observations

- Cursor jumps to document end mid-typing
- Only happens during rapid typing (keyboard mashing)
- Does NOT happen when pasting large text
- Independent of scroll sync setting
- Reproducible in single-user sessions

### Data Flow Analysis

```
LOCAL CHANGE:
User types → Monaco onChange → handleEditorChange → onContentChange (App.tsx)
                                    ↓
                           updateFileContent (automergeSync)
                                    ↓
                           Automerge handle.change()
                                    ↓
                           callbacks.onFileChanged(path, content, patches)
                                    ↓
                           setFileContents

REACT EFFECT (Editor.tsx):
When fileContents changes:
  - Compare Monaco model content with Automerge content
  - If different: compute diff, apply via executeEdits()
  - Call setContent() to sync React state for preview
```

### Hypothesis 1: Patch-Based Sync Race Condition (PARTIALLY CORRECT)

**Theory:** During rapid typing, React's `content` state could be stale compared to Monaco's actual model when calculating patch positions.

**Action:** Implemented diff-based synchronization using `fast-diff` library instead of relying on Automerge patches. This computes edits directly from Monaco's current content to Automerge's authoritative content.

**Result:** Bug persisted. The diff-based approach was correct but not sufficient.

### Hypothesis 2: setContent() After executeEdits() (PARTIALLY CORRECT)

**Theory:** The `setContent(automergeContent)` call after `executeEdits()` triggers a React re-render, which might cause the wrapper to reset cursor.

**Debug logging added:**
```typescript
const cursorBefore = editorRef.current?.getPosition();
editorRef.current.executeEdits('remote-sync', edits);
const cursorAfter = editorRef.current.getPosition();
// Log showed cursor was correct after executeEdits

setTimeout(() => {
  const cursorAfterSetContent = editorRef.current?.getPosition();
  // Log showed cursor MOVED after setContent!
}, 0);
```

**Finding:** Cursor was stable after `executeEdits()` but moved after `setContent()`.

**Action:** Skip `setContent()` when Monaco already matches Automerge after edits.

**Result:** Bug persisted. Console went silent (no warnings), but cursor still jumped.

### Hypothesis 3: Controlled Component Value Prop Mismatch (CORRECT)

**Theory:** Even though we skip `setContent()` in the sync effect, this creates a new problem:
- Monaco model has new content (after executeEdits)
- React `content` state has OLD content (we skipped setContent)
- On next render, `value={content}` has old content
- @monaco-editor/react sees `value` != model, does setValue(value) → cursor reset!

The wrapper's controlled mode continuously tries to sync the `value` prop with Monaco's model. Any divergence triggers a `setValue()`.

**Action:** Switch to uncontrolled mode:
1. Change `value={content}` to `defaultValue={content}`
2. Add `key={currentFile?.path}` to force remount on file switch
3. Now `setContent()` only updates React state for preview, doesn't affect Monaco

**Result:** Bug fixed.

### Key Insight

The `@monaco-editor/react` wrapper in controlled mode (`value` prop) actively enforces that Monaco's content matches the prop value. This is problematic when:
1. External state (Automerge) updates asynchronously
2. You want to apply changes via `executeEdits()` to preserve cursor
3. React state can temporarily diverge from Monaco's model

Uncontrolled mode (`defaultValue` prop) lets Monaco manage its own content. We can still:
- Get initial/file-switch content via `defaultValue`
- Apply remote changes via `executeEdits()`
- Track content in React state for preview (without affecting Monaco)

### Why Cursor Jumped to END Specifically

Monaco's `setValue()` method resets cursor to the end of the document. This is the characteristic signature of the wrapper doing a full content replacement rather than incremental edits.

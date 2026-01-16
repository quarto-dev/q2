# Plan: Preview Link Click Refinements

**Issue**: kyoto-ksw
**Status**: Draft
**Created**: 2026-01-16

## Goal

Refine the HTML preview's link click handling to support:
1. Links to non-existent .qmd files → prompt to create the file
2. Anchor links within the current document → scroll to anchor
3. Links to other documents with anchors → switch document + scroll to anchor
4. External links (https://...) → open in new tab

## Current State

### How .qmd links work now

**File**: `hub-client/src/utils/iframePostProcessor.ts` (lines 78-92)

```typescript
// Current implementation - only handles existing files
doc.querySelectorAll('a[href$=".qmd"]').forEach((anchor) => {
  const href = anchor.getAttribute('href');
  if (href) {
    const targetPath = resolveRelativePath(options.currentFilePath, href);
    anchor.addEventListener('click', (e) => {
      e.preventDefault();
      options.onQmdLinkClick!(targetPath);  // Just passes the path
    });
  }
});
```

**File**: `hub-client/src/components/Editor.tsx` (lines 245-256)

```typescript
// Current callback - only navigates if file exists
const handleQmdLinkClick = useCallback(
  (targetPath: string) => {
    const file = files.find(
      (f) => f.path === targetPath || '/' + f.path === targetPath
    );
    if (file) {
      setCurrentFile(file);  // Only works for existing files
    }
    // Non-existent files are silently ignored
  },
  [files]
);
```

### How anchor links work now

Currently, anchor links (`#section`) are NOT intercepted. They work as normal browser navigation within the iframe, scrolling to the anchor.

### How scroll sync works

**File**: `hub-client/src/hooks/useScrollSync.ts`

The scroll sync hook listens for:
- **scroll events** on the iframe's contentWindow (line 231)
- **click events** on the iframe's contentDocument (line 233)

When the preview scrolls, `syncPreviewToEditor()` is called (after 50ms debounce), which uses **scroll ratio matching** to scroll the editor to the same proportional position.

**Key insight**: When we programmatically scroll the preview (via `scrollIntoView`), the scroll event will fire automatically, triggering the existing scroll sync mechanism. We don't need to manually scroll the editor - the existing infrastructure handles it.

### How new file creation works

**File**: `hub-client/src/components/NewFileDialog.tsx`

- Has an `initialFiles` prop for drag-drop pre-population
- Has a controlled `filename` state
- Currently no way to pre-populate just the filename string

## Design Decisions

### 1. Non-existent file links

**Decision**: Add a new prop `initialFilename?: string` to NewFileDialog
- Dialog opens with filename pre-filled, user can edit and confirm
- Safe - no automatic file creation on accidental clicks or typos

### 2. Anchor scrolling in current document

**Decision**: Just scroll the preview; let scroll sync naturally update the editor
- Uses existing scroll sync mechanism (triggered by scroll event)
- Scroll sync uses ratio-based positioning (not line-precise), which is consistent with existing behavior
- Simple implementation - no manual editor scrolling needed

### 3. Cross-document anchor links

**Decision**: Store pending anchor in state, apply after render completes
- Clean React state management pattern
- Need to add new state variable and effect to apply anchor after iframe loads

### 4. External links

**Decision**: Add `target="_blank"` and `rel="noopener noreferrer"` to external links
- Prevents navigation inside the iframe
- Standard security practice for external links

## Implementation Plan

### Phase 1: Parse anchors and handle external links

**File**: `hub-client/src/utils/iframePostProcessor.ts`

1. Update link handling to cover more cases:
   - `.qmd` links (with or without anchors)
   - Same-document anchor links (`#section`)
   - External links (`http://`, `https://`)

2. Parse href into components:
   ```typescript
   interface ParsedLink {
     path: string | null;     // null for same-document anchors
     anchor: string | null;   // null if no anchor
   }

   function parseQmdLink(href: string): ParsedLink {
     // Handle anchors: "file.qmd#section" or "#section"
     const hashIndex = href.indexOf('#');
     if (hashIndex === -1) {
       return { path: href, anchor: null };
     }
     const path = hashIndex === 0 ? null : href.substring(0, hashIndex);
     const anchor = href.substring(hashIndex + 1);
     return { path, anchor: anchor || null };
   }
   ```

3. Update callback signature:
   ```typescript
   onQmdLinkClick?: (targetPath: string | null, anchor: string | null) => void;
   ```

4. Handle external links:
   ```typescript
   // Open external links in new tab
   doc.querySelectorAll('a[href^="http://"], a[href^="https://"]').forEach((anchor) => {
     anchor.setAttribute('target', '_blank');
     anchor.setAttribute('rel', 'noopener noreferrer');
   });
   ```

### Phase 2: Update NewFileDialog

**File**: `hub-client/src/components/NewFileDialog.tsx`

1. Add optional `initialFilename?: string` prop
2. Initialize filename state from this prop when provided
3. Reset when dialog opens with new initial value

### Phase 3: Update Editor link handler

**File**: `hub-client/src/components/Editor.tsx`

1. Add state for pending anchor:
   ```typescript
   const [pendingAnchor, setPendingAnchor] = useState<string | null>(null);
   ```

2. Add state for triggering new file dialog with pre-populated name:
   ```typescript
   const [newFileInitialName, setNewFileInitialName] = useState<string>('');
   ```

3. Update `handleQmdLinkClick` to handle all cases:
   ```typescript
   const handleQmdLinkClick = useCallback(
     (targetPath: string | null, anchor: string | null) => {
       // Case 1: Same-document anchor only
       if (!targetPath && anchor) {
         scrollToAnchor(anchor);
         return;
       }

       // Case 2: Link to a different document
       if (targetPath) {
         const file = files.find(f => f.path === targetPath || '/' + f.path === targetPath);

         if (file) {
           // Existing file - switch to it
           setCurrentFile(file);
           if (anchor) {
             setPendingAnchor(anchor);  // Will scroll after render
           }
         } else {
           // Non-existent file - open create dialog
           setNewFileInitialName(targetPath);
           setShowNewFileDialog(true);
         }
       }
     },
     [files, scrollToAnchor]
   );
   ```

4. Add effect to handle pending anchor after render:
   ```typescript
   useEffect(() => {
     if (pendingAnchor && !isRendering) {
       // Wait for iframe to fully load new content
       const timer = setTimeout(() => {
         scrollToAnchor(pendingAnchor);
         setPendingAnchor(null);
       }, 100);
       return () => clearTimeout(timer);
     }
   }, [pendingAnchor, isRendering, scrollToAnchor]);
   ```

5. Add `scrollToAnchor` function:
   ```typescript
   const scrollToAnchor = useCallback((anchor: string) => {
     const iframeRef = activeIframe === 'A' ? iframeARef : iframeBRef;
     const doc = iframeRef.current?.contentDocument;
     if (!doc) return;

     const element = doc.getElementById(anchor);
     if (element) {
       element.scrollIntoView({ behavior: 'smooth', block: 'start' });
       // Scroll sync will automatically update the editor via scroll event listener
     }
     // If element doesn't exist, do nothing (no-op as requested)
   }, [activeIframe]);
   ```

### Phase 4: Wire up NewFileDialog

1. Pass `initialFilename` prop to NewFileDialog:
   ```tsx
   <NewFileDialog
     isOpen={showNewFileDialog}
     existingPaths={files.map(f => f.path)}
     initialFilename={newFileInitialName}
     onClose={() => {
       handleDialogClose();
       setNewFileInitialName('');  // Clear on close
     }}
     onCreateTextFile={handleCreateTextFile}
     onUploadBinaryFile={handleUploadBinaryFile}
     initialFiles={pendingUploadFiles}
   />
   ```

## Testing Plan

### Manual Testing

1. **Non-existent file link**:
   - Create a document with `[link](new-file.qmd)`
   - Click the link
   - Verify: NewFileDialog opens with "new-file.qmd" pre-filled
   - Verify: Can edit the name and create the file
   - Verify: After creation, switched to new file

2. **Anchor link in same document**:
   - Create a document with headings and `[link](#heading-id)`
   - Click the link
   - Verify: Preview scrolls to the heading
   - Verify: If scroll sync enabled, editor scrolls too (via automatic scroll event)

3. **Anchor link that doesn't exist**:
   - Create a document with `[link](#nonexistent)`
   - Click the link
   - Verify: Nothing happens (no-op)

4. **Cross-document link with anchor**:
   - Create doc A with `[link](doc-b.qmd#section)`
   - Create doc B with a `## Section` heading
   - From doc A, click the link
   - Verify: Switches to doc B
   - Verify: Scrolls to the Section heading

5. **Relative path resolution**:
   - Create `docs/guide.qmd` with `[link](../readme.qmd)`
   - Verify: Resolves to `readme.qmd` correctly

6. **External links**:
   - Create a document with `[link](https://example.com)`
   - Click the link
   - Verify: Opens in a new browser tab
   - Verify: Does NOT navigate the iframe

## Files to Modify

1. `hub-client/src/utils/iframePostProcessor.ts` - Parse anchors, handle external links
2. `hub-client/src/components/NewFileDialog.tsx` - Add initialFilename prop
3. `hub-client/src/components/Editor.tsx` - Update handler, add pending anchor state
4. `hub-client/src/hooks/useIframePostProcessor.ts` - Update options type

## Complexity Assessment

- Phase 1: Small (parsing changes, external link handling)
- Phase 2: Small (one prop addition)
- Phase 3: Medium (new state, effects, handler logic)
- Phase 4: Small (wiring)

Total: Moderate complexity, focused changes

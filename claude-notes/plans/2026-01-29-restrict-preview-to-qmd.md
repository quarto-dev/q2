# Plan: Restrict Preview and QMD Features to .qmd Files Only

**Issue**: kyoto-xem
**Date**: 2026-01-29

## Overview

Currently, hub-client treats ALL text files as QMD files, attempting to parse and preview them via WASM. This causes:
- Render errors for non-qmd files (CSS, JSON, YAML, etc.)
- Unnecessary WASM processing
- Confusing diagnostics for non-qmd content

This plan implements proper file type detection so that only `.qmd` files receive QMD-specific features (preview, diagnostics, folding, outline), while other text files remain editable in Monaco with Automerge sync.

## Current State

### What already works correctly
- **monacoProviders.ts** (lines 181, 208): Already checks `path?.endsWith('.qmd')` before providing symbols and folding ranges
- **File sidebar**: Correctly distinguishes binary from text files
- **Monaco editor**: Works fine for all text files

### What needs to change
1. **Preview.tsx**: Always renders content through WASM regardless of file type
2. **useIntelligence.ts**: Calls `analyzeDocument()` for any file path
3. **Editor.tsx**: Always shows the preview pane regardless of file type

## Design Decisions

### Option A: Hide preview pane entirely for non-qmd files
- Pros: Clear UX - if there's no preview, don't show the pane
- Cons: Layout shift when switching between file types; more complex state management

### Option B: Show empty/placeholder state in preview pane for non-qmd files ✓ RECOMMENDED
- Pros: Consistent layout; clear messaging; simpler implementation
- Cons: Takes up space that could be used for editor

**Recommendation**: Option B - show a placeholder message like "Preview available for .qmd files" in the preview pane. This maintains a consistent layout and provides clear feedback.

### Helper function location
Add `isQmdFile(path: string): boolean` to `src/types/project.ts` for consistency with other file type helpers.

## Work Items

- [x] Add `isQmdFile()` helper function to `src/types/project.ts`
- [x] Update `Preview.tsx` to show placeholder for non-qmd files
- [x] Update `useIntelligence.ts` to skip analysis for non-qmd files
- [x] Update `intelligenceService.ts` to guard against non-qmd files (defense in depth)
- [x] Test behavior with various file types (.qmd, .css, .json, .yml, .md, .tsx)
- [ ] Update hub-client changelog

## Implementation Details

### 1. Add `isQmdFile()` helper (`src/types/project.ts`)

```typescript
/**
 * Check if a file path represents a QMD file.
 */
export function isQmdFile(path: string | null | undefined): boolean {
  return path?.toLowerCase().endsWith('.qmd') ?? false;
}
```

### 2. Update `Preview.tsx`

Add early return for non-qmd files that renders a placeholder instead of WASM content.

```typescript
// At the start of the component, after getting currentFile
const isQmd = isQmdFile(currentFile?.path);

// In the render, show placeholder for non-qmd files
if (!isQmd) {
  return (
    <div className="pane preview-pane preview-placeholder">
      <div className="preview-placeholder-content">
        <p>Preview available for .qmd files</p>
      </div>
    </div>
  );
}
```

Key changes:
- Import `isQmdFile` from `../types/project`
- Add check before rendering
- Skip WASM initialization/rendering for non-qmd files
- Clear diagnostics when switching to non-qmd file (already happens via `onDiagnosticsChange([])`)

### 3. Update `useIntelligence.ts`

Add early return in `doAnalyze()` for non-qmd files:

```typescript
const doAnalyze = useCallback(async () => {
  if (!path) {
    // ... existing null path handling
  }

  // Only analyze .qmd files
  if (!isQmdFile(path)) {
    setSymbols([]);
    setDiagnostics([]);
    setFoldingRanges([]);
    setError(null);
    setLoading(false);
    return;
  }

  // ... rest of existing logic
}, [path, enableSymbols, enableDiagnostics, enableFoldingRanges]);
```

### 4. Update `intelligenceService.ts` (defense in depth)

Add guards to `getSymbols()`, `getFoldingRanges()`, and `analyzeDocument()`:

```typescript
export async function getSymbols(path: string): Promise<Symbol[]> {
  if (!isQmdFile(path)) return [];
  // ... existing logic
}

export async function getFoldingRanges(path: string): Promise<FoldingRange[]> {
  if (!isQmdFile(path)) return [];
  // ... existing logic
}

export async function analyzeDocument(path: string): Promise<DocumentAnalysis> {
  if (!isQmdFile(path)) {
    return { symbols: [], diagnostics: [], foldingRanges: [] };
  }
  // ... existing logic
}
```

### 5. CSS for placeholder

Add to `Editor.css` or appropriate stylesheet:

```css
.preview-placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--vscode-editor-background);
  color: var(--vscode-descriptionForeground);
}

.preview-placeholder-content {
  text-align: center;
  font-size: 14px;
}
```

## Testing Plan

1. **QMD files**: Verify preview, diagnostics, folding, and outline all work as before
2. **CSS files**: Verify no preview, no diagnostics, still editable
3. **JSON files**: Verify no preview, no diagnostics, still editable
4. **YAML files**: Verify no preview, no diagnostics, still editable
5. **Markdown (.md) files**: Verify no preview (not .qmd), no diagnostics, still editable
6. **TypeScript/JavaScript files**: Verify no preview, no diagnostics, still editable
7. **File switching**: Verify switching between qmd and non-qmd files works smoothly
8. **Automerge sync**: Verify all files still sync correctly

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Users expect .md files to render | The placeholder message is clear. Future enhancement could add markdown preview. |
| Performance regression from extra checks | `endsWith()` is O(n) where n is extension length - negligible |
| Breaking existing workflows | Changes are additive - qmd files work exactly as before |

## Out of Scope

- Syntax highlighting for different file types (Monaco defaults to markdown for all)
- Preview for non-qmd formats (e.g., rendering markdown, showing JSON trees)
- Language-specific features (TypeScript intellisense, etc.)

These could be future enhancements tracked as separate issues.

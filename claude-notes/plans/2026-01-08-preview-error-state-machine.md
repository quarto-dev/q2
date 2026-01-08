# Preview Pane Error State Machine

**Beads Issue**: k-nwcy
**Date**: 2026-01-08
**Status**: Implemented (pending manual testing)

## Design Decisions (Resolved)

1. **Overlay Style**: Option A - Toast notification (bottom-right corner). Keep other alternatives in plan for potential future experimentation.

2. **Error Detail Level**: Show full error message with:
   - Maximum 20 lines visible initially
   - Internal scrollbar for longer messages
   - Format similar to current error display

3. **Dismissibility**: Collapsible but not fully dismissible while in ERROR_FROM_GOOD state
   - Users can collapse to a minimal indicator
   - Can expand back to see full error
   - Cannot fully hide while errors exist

4. **Multiple Errors**: Display like current error message format (all errors shown). Future enhancement: make errors interactive/linked to source.

## Problem Statement

When editing a `.qmd` file in hub-client, temporarily introducing a syntax error causes an unpleasant visual flash. The preview pane replaces the rendered HTML with an error page showing:
1. A red "Render Error" message box
2. The raw unformatted markdown source

This behavior is jarring during normal editing flow when the error is transient (e.g., mid-keystroke while typing a link or code fence).

## Proposed Solution

Implement a 4-state machine for the preview pane:

```
             ┌──────────────────────────────────────┐
             │                                      │
             ▼                error                 │
          ┌─────┐ ─────────────────────► ┌────────────────┐
          │START│                        │ERROR_AT_START  │
          └─────┘                        └────────────────┘
             │                                  │ ▲
             │ good                       good  │ │ error
             │                                  ▼ │
             │                           ┌────────────────┐
             └──────────────────────────►│     GOOD       │
                                         └────────────────┘
                                                │ ▲
                                          error │ │ good
                                                ▼ │
                                         ┌────────────────┐
                                         │ERROR_FROM_GOOD │◄──┐
                                         └────────────────┘   │ error
                                                │             │
                                                └─────────────┘
```

### State Descriptions

| State | Preview Content | Error Display |
|-------|-----------------|---------------|
| **START** | Blank/loading | None |
| **ERROR_AT_START** | Error page with raw source | Full page error (current behavior) |
| **GOOD** | Rendered HTML | None (diagnostics in editor only) |
| **ERROR_FROM_GOOD** | Last good rendered HTML | Overlay notification |

### Transition Rules

| From | Event | To | Action |
|------|-------|-----|--------|
| START | error | ERROR_AT_START | Show full error page |
| START | good | GOOD | Show rendered HTML |
| ERROR_AT_START | error | ERROR_AT_START | Update error page |
| ERROR_AT_START | good | GOOD | Show rendered HTML |
| GOOD | error | ERROR_FROM_GOOD | Keep HTML, show overlay |
| GOOD | good | GOOD | Update rendered HTML |
| ERROR_FROM_GOOD | error | ERROR_FROM_GOOD | Update overlay only |
| ERROR_FROM_GOOD | good | GOOD | Hide overlay, update HTML |

## Current Architecture

### Key Files
- `hub-client/src/components/Editor.tsx` - Main preview logic
- `hub-client/src/types/diagnostic.ts` - Diagnostic type definitions

### Current Flow
1. Content changes trigger `doRender()` (debounced 300ms)
2. `renderToHtml()` returns `RenderResponse` with `success`, `html`, `diagnostics`
3. On success: HTML goes to inactive iframe, then swaps
4. On error: `renderError()` generates error HTML, goes to inactive iframe, then swaps

### Double-Buffering System
The preview already uses double-buffering with two iframes (A and B) to prevent flash during successful renders. The issue is that error HTML is still swapped in, replacing the good content.

## Implementation Plan

### Step 1: Add Preview State Enum

Add to `Editor.tsx`:

```typescript
type PreviewState = 'START' | 'ERROR_AT_START' | 'GOOD' | 'ERROR_FROM_GOOD';
```

Add new state:
```typescript
const [previewState, setPreviewState] = useState<PreviewState>('START');
const [lastGoodHtml, setLastGoodHtml] = useState<string | null>(null);
const [currentError, setCurrentError] = useState<{message: string, diagnostics?: Diagnostic[]} | null>(null);
```

### Step 2: Modify doRender() Logic

Current logic (simplified):
```typescript
if (result.success) {
  setInactiveHtml(result.html);
  setSwapPending(true);
} else {
  setInactiveHtml(renderError(content, result.error, diagnostics));
  setSwapPending(true);
}
```

New logic:
```typescript
if (result.success) {
  setLastGoodHtml(result.html);
  setCurrentError(null);
  setInactiveHtml(result.html);
  setSwapPending(true);

  // Transition to GOOD from any state
  setPreviewState('GOOD');
} else {
  setCurrentError({ message: result.error, diagnostics: result.diagnostics });

  if (previewState === 'START' || previewState === 'ERROR_AT_START') {
    // No good render yet - show full error page
    setInactiveHtml(renderError(content, result.error, diagnostics));
    setSwapPending(true);
    setPreviewState('ERROR_AT_START');
  } else {
    // Was GOOD or ERROR_FROM_GOOD - keep last good HTML, show overlay
    // DON'T swap iframes, DON'T change HTML content
    setPreviewState('ERROR_FROM_GOOD');
  }
}
```

### Step 3: Add Error Overlay Component

Create `hub-client/src/components/PreviewErrorOverlay.tsx`:

```typescript
import { useState } from 'react';
import { Diagnostic } from '../types/diagnostic';

interface PreviewErrorOverlayProps {
  error: { message: string; diagnostics?: Diagnostic[] } | null;
  visible: boolean;
}

export function PreviewErrorOverlay({ error, visible }: PreviewErrorOverlayProps) {
  const [collapsed, setCollapsed] = useState(false);

  if (!visible || !error) return null;

  if (collapsed) {
    // Collapsed state: minimal indicator
    return (
      <div className="preview-error-overlay preview-error-overlay--collapsed">
        <button
          className="preview-error-expand-btn"
          onClick={() => setCollapsed(false)}
          title="Show error details"
        >
          ⚠ Error
        </button>
      </div>
    );
  }

  // Expanded state: full error toast
  return (
    <div className="preview-error-overlay preview-error-overlay--expanded">
      <div className="preview-error-header">
        <span className="preview-error-title">⚠ Render Error</span>
        <button
          className="preview-error-collapse-btn"
          onClick={() => setCollapsed(true)}
          title="Collapse"
        >
          −
        </button>
      </div>
      <div className="preview-error-content">
        {/* Max 20 lines visible, scrollable */}
        <pre className="preview-error-message">
          {error.message}
        </pre>
        {error.diagnostics && error.diagnostics.length > 0 && (
          <ul className="preview-error-diagnostics">
            {error.diagnostics.map((d, i) => (
              <li key={i}>
                {d.start_line && `Line ${d.start_line}: `}
                {d.title}
                {d.problem && ` - ${d.problem}`}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
```

**CSS styling** (add to `Editor.css`):

```css
.preview-error-overlay {
  position: absolute;
  bottom: 16px;
  right: 16px;
  z-index: 1000;
  background: #1e1e1e;
  border: 1px solid #f44336;
  border-radius: 8px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  font-family: system-ui, -apple-system, sans-serif;
  font-size: 13px;
  color: #e0e0e0;
}

.preview-error-overlay--collapsed {
  padding: 8px 12px;
}

.preview-error-overlay--expanded {
  max-width: 400px;
  min-width: 280px;
}

.preview-error-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 12px;
  border-bottom: 1px solid #333;
  background: #2d2d2d;
  border-radius: 8px 8px 0 0;
}

.preview-error-title {
  color: #f44336;
  font-weight: 600;
}

.preview-error-collapse-btn,
.preview-error-expand-btn {
  background: none;
  border: none;
  color: #999;
  cursor: pointer;
  font-size: 16px;
  padding: 0 4px;
}

.preview-error-collapse-btn:hover,
.preview-error-expand-btn:hover {
  color: #fff;
}

.preview-error-content {
  padding: 12px;
  max-height: calc(20 * 1.4em); /* ~20 lines */
  overflow-y: auto;
}

.preview-error-message {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: 'SF Mono', Monaco, 'Cascadia Code', monospace;
  font-size: 12px;
  line-height: 1.4;
}

.preview-error-diagnostics {
  margin: 8px 0 0 0;
  padding-left: 20px;
  list-style: disc;
}

.preview-error-diagnostics li {
  margin: 4px 0;
}
```

### Step 4: Integrate Overlay in Editor.tsx

Add overlay alongside preview iframes:

```tsx
<div className="preview-container">
  <iframe ... /> {/* Active iframe A */}
  <iframe ... /> {/* Inactive iframe B */}
  <PreviewErrorOverlay
    error={currentError}
    visible={previewState === 'ERROR_FROM_GOOD'}
  />
</div>
```

### Step 5: Handle File Changes

When switching files, reset to START state:
```typescript
// In the file switch handler (around line 487)
setPreviewState('START');
setLastGoodHtml(null);
setCurrentError(null);
```

## Design Considerations for Error Overlay

### Option A: Toast Notification (Bottom-right corner)

```
┌──────────────────────────────────────────┐
│                                          │
│          (Good HTML Preview)             │
│                                          │
│                                          │
│                        ┌───────────────┐ │
│                        │ ⚠ Syntax Error│ │
│                        │ Line 12: ...  │ │
│                        │ [×]           │ │
│                        └───────────────┘ │
└──────────────────────────────────────────┘
```

**Pros**: Non-intrusive, common UX pattern
**Cons**: Might be missed, limited space for details

### Option B: Top Banner with Collapse

```
┌──────────────────────────────────────────┐
│ ⚠ Parse error at line 12   [Details ▼]  │
├──────────────────────────────────────────┤
│                                          │
│          (Good HTML Preview)             │
│                                          │
└──────────────────────────────────────────┘
```

**Pros**: Always visible, expandable for details
**Cons**: Takes vertical space, similar to existing diagnostic banner

### Option C: Semi-transparent Overlay Bar (Bottom)

```
┌──────────────────────────────────────────┐
│                                          │
│          (Good HTML Preview)             │
│                                          │
│▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│
│ ⚠ Syntax error on line 12: Unclosed... │
└──────────────────────────────────────────┘
```

**Pros**: Visible but doesn't obscure much content
**Cons**: Might cover important preview content

### Option D: Border Glow + Corner Badge

```
┌──────────────────────────────────────────┐
│⚠ 1 error                                 │
│                                          │
│          (Good HTML Preview)             │
│            with red border               │
│                                          │
└──────────────────────────────────────────┘
```

**Pros**: Minimal visual intrusion, clear signal
**Cons**: Less detail immediately visible

## Open Questions (for future iteration)

1. **Animation**: Should the overlay animate in/out? (fade, slide) - defer to implementation
2. **Future enhancement**: Make errors interactive/clickable to jump to source location

## Testing Plan

1. **Unit Tests**
   - State machine transitions (all 8 transitions)
   - Error overlay visibility logic

2. **Integration Tests**
   - Edit file → introduce error → verify HTML preserved
   - Edit file → fix error → verify new HTML renders
   - Switch files → verify state resets

3. **Manual Testing Scenarios**
   - Type incomplete link `[text](` → verify no flash
   - Type incomplete code fence → verify no flash
   - Multiple rapid edits with transient errors
   - Long document with scroll position → verify scroll preserved

## Implementation Order

1. Add state tracking (`previewState`, `lastGoodHtml`, `currentError`)
2. Modify `doRender()` with new logic
3. Create `PreviewErrorOverlay` component (placeholder styling)
4. Integrate overlay in Editor.tsx
5. Add file-switch reset logic
6. Style the overlay (after design decision)
7. Add tests

## Estimated Complexity

- **State management changes**: Low complexity, localized to Editor.tsx
- **Overlay component**: Medium complexity, needs design iteration
- **Testing**: Medium complexity, need to simulate render errors

## Related Files

- `hub-client/src/components/Editor.tsx` (primary changes)
- `hub-client/src/components/PreviewErrorOverlay.tsx` (new file)
- `hub-client/src/types/diagnostic.ts` (reference, no changes)
- `hub-client/src/components/Editor.css` (overlay styling)

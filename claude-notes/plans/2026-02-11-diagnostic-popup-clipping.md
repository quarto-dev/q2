# Fix Diagnostic Popup Clipping by Navbar (bd-1wxq)

## Overview

Monaco editor diagnostic hover popups get clipped when they appear near the top (or bottom) of the editor viewport. This happens because `.editor-main` uses `overflow: hidden`, and Monaco positions its hover widgets absolutely within the editor container.

**Screenshot evidence**: A diagnostic on line 6 (near top of editor) shows the "Unknown shortcode" hover popup extending above the visible area, getting clipped by the navbar.

## Root Cause Analysis

### Layout hierarchy
```
.editor-container (height: 100vh, flex column)
├── MinimalHeader (min-height: 36px, padding: 8px 16px → ~52px total)
├── .diagnostics-banner (optional)
└── .editor-main (flex: 1, overflow: hidden)  ← CLIPS CONTENT
    ├── .editor-pane (position: relative)
    │   └── MonacoEditor (height: 100%)
    └── Preview iframe
```

### Why clipping occurs
1. `.editor-main` has `overflow: hidden` (Editor.css:180) — this creates a clipping boundary
2. Monaco positions hover widgets absolutely within its container
3. When a hover popup appears above a line near the top of the editor, it extends past the `.editor-main` boundary and gets clipped
4. The navbar sits above `.editor-main`, so the clipped area is hidden behind it

### Current Monaco options (Editor.tsx:718-728)
```typescript
options={{
  minimap: { enabled: false },
  fontSize: 14,
  lineNumbers: 'on',
  wordWrap: 'on',
  padding: { top: 16 },
  scrollBeyondLastLine: false,
  pasteAs: { enabled: false },
}}
```

No `fixedOverflowWidgets`, `hover`, or tooltip-related options are configured.

## Approach Options

### Option A: `fixedOverflowWidgets: true` (simplest)
Monaco has a built-in `fixedOverflowWidgets` option that moves overflow widgets (including hover/diagnostics) to a fixed-position container at the document body level, outside the editor's overflow boundary.

**Pros**: One-line fix, built-in Monaco feature, well-tested
**Cons**: The widget is now positioned relative to the viewport — may still overlap the navbar. Needs CSS to constrain the fixed widgets to respect the navbar area.

### Option B: Constrain hover direction based on position (user-preferred approach)
Ensure the diagnostic popup is shifted vertically so it stays within visible bounds:
- Near top of editor → show popup **below** the diagnostic line
- Near bottom of editor → show popup **above** the diagnostic line

Monaco's `hover` option has an `above` property (boolean) that controls default placement. However, this is a static setting and doesn't dynamically switch based on cursor/line position.

A more robust approach would combine:
1. `fixedOverflowWidgets: true` to break out of the `overflow: hidden` container
2. CSS clamping via `max()` / `min()` on the widget's `top` property to ensure it stays below the navbar and above the bottom edge

### Option C: CSS-only fix with overflow adjustment
Change `.editor-main` from `overflow: hidden` to `overflow: clip` or remove it, and use CSS to constrain Monaco widgets.

**Pros**: No JS changes
**Cons**: `overflow: hidden` may be intentional for layout reasons; removing it could cause other issues.

## Recommended Approach

Combine Option A + CSS constraints (a variant of Option B):

1. Add `fixedOverflowWidgets: true` to Monaco options — this prevents the `overflow: hidden` clipping
2. Add CSS that constrains `.monaco-editor .overflow-guard > .overflowingContentWidgets` (or the relevant fixed widget container) to stay within the viewport minus the navbar height
3. Test with diagnostics at various positions (top, middle, bottom of editor)

## Work Items

- [x] Add `fixedOverflowWidgets: true` to Monaco editor options in Editor.tsx
- [x] Add `hover: { above: false }` to prefer showing hovers below the line
- [ ] Test diagnostics near the top of the editor (manual)
- [ ] Test diagnostics near the bottom of the editor (manual)
- [ ] Test diagnostics in the middle of the editor (regression check, manual)
- [ ] Verify presence cursors and other hover widgets are not affected (manual)

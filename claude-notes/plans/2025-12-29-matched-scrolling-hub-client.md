# Matched Scrolling for Hub-Client

**Issue:** k-suww
**Date:** 2025-12-29
**Status:** Planning

## Overview

Implement bidirectional scroll synchronization between the Monaco editor and the HTML preview pane in hub-client.

**Prerequisite:** k-ic1o (ConfigValue integration into render pipeline) - provides the infrastructure to inject source location settings into project configuration.

**Related design documents:**
- `claude-notes/plans/2025-12-07-config-merging-design.md` - Full config merging design
- `claude-notes/plans/2025-12-29-config-integration-pipeline.md` - Minimal implementation for this feature

This allows users to:
1. Have the preview automatically scroll to show content corresponding to the editor cursor position
2. Have the editor viewport scroll to match manual scrolling in the preview pane

## Background

### Current Architecture

**Hub-client rendering pipeline:**
1. User types in Monaco editor
2. Content changes trigger `updatePreview()` with 300ms debounce
3. `wasmRenderer.renderToHtml()` calls WASM module
4. WASM uses unified `quarto-core` pipeline (`render_qmd_to_html`)
5. HTML is set as iframe's `srcDoc`
6. Post-processor converts resource links to data URIs

**Source location tracking in pampa:**
- HTML writer supports `data-sid` and `data-loc` attributes
- Enabled via document metadata: `format.html.source-location: full`
- `data-loc` format: `file_id:start_line:start_col-end_line:end_col` (1-based)
- Inline text wrapped in `<span>` elements with location data
- Block elements (p, h1-h6, div, etc.) also get location attributes

**Relevant files:**
- `hub-client/src/components/Editor.tsx` - Main editor component
- `hub-client/src/services/wasmRenderer.ts` - WASM rendering interface
- `crates/wasm-quarto-hub-client/src/lib.rs` - WASM entry points
- `crates/quarto-core/src/pipeline.rs` - Unified render pipeline
- `crates/pampa/src/writers/html.rs` - HTML writer with source tracking

## Requirements

### Functional Requirements

1. **Editor → Preview sync (cursor-driven)**
   - When editor cursor moves to a new line, preview scrolls to show corresponding output
   - Scroll only if the target element is not already visible (avoid unnecessary movement)
   - 50ms debounce on cursor position changes

2. **Preview → Editor sync (scroll-driven)**
   - When user scrolls preview, editor viewport scrolls to match
   - Editor cursor position does NOT change (only viewport)
   - 50ms debounce on scroll events

3. **UI Toggle**
   - Single toggle in toolbar header area
   - Controls both sync directions simultaneously
   - Default: off (to avoid surprising users)

4. **Pipeline Integration**
   - Hub-client injects `format.html.source-location: full` into document metadata
   - Rendering pipeline shared between `quarto render` and hub-client
   - No code duplication; hub-client adds a transform step

### Non-Functional Requirements

- No jittery scrolling behavior
- Smooth scroll animations
- Minimal performance impact
- Acceptable HTML size overhead from inline source tracking

## Design

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Hub-Client                                │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌──────────────┐    ┌─────────────────────┐ │
│  │   Monaco    │◄──►│  useScroll   │◄──►│   Preview iframe    │ │
│  │   Editor    │    │    Sync      │    │   (with data-loc)   │ │
│  └─────────────┘    └──────────────┘    └─────────────────────┘ │
│         │                  │                      │              │
│         ▼                  ▼                      ▼              │
│  cursor position    debounced sync         scroll position      │
│  (line, column)     coordination           element → line       │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     WASM Renderer                                │
├─────────────────────────────────────────────────────────────────┤
│  renderQmdContent(content, templateBundle)                       │
│       │                                                          │
│       ▼                                                          │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │  Pipeline Transform: inject source-location metadata        ││
│  │  (hub-client specific, before render)                       ││
│  └─────────────────────────────────────────────────────────────┘│
│       │                                                          │
│       ▼                                                          │
│  Unified quarto-core pipeline (same as `quarto render`)         │
│       │                                                          │
│       ▼                                                          │
│  HTML with data-loc attributes                                   │
└─────────────────────────────────────────────────────────────────┘
```

### Source Location Injection

**Approach: Project Configuration Injection** (see k-ic1o)

Rather than injecting metadata directly, we use the proper configuration merging infrastructure:

1. WASM creates a `ProjectConfig` with `format_config` containing `format.html.source-location: full`
2. `render_qmd_to_html` merges project config with document metadata
3. HTML writer reads the merged config and sees `source-location: full`
4. Document metadata can still override if user explicitly sets the value

This approach:
- Preserves source integrity (no document modification)
- Uses the planned configuration infrastructure
- Allows future project-level settings with the same pattern
- Enables documents to override project defaults when needed

See `claude-notes/plans/2025-12-29-config-integration-pipeline.md` for implementation details.

### Scroll Sync Algorithm

**Editor → Preview:**
```
1. Listen to Monaco onDidChangeCursorPosition
2. Debounce 50ms
3. Get current cursor line number
4. Query preview iframe for element with matching data-loc
   - Find element where start_line <= cursor_line <= end_line
   - Prefer most specific (smallest range) match
5. Check if element is in viewport
6. If not visible, scrollIntoView({ behavior: 'smooth', block: 'nearest' })
```

**Preview → Editor:**
```
1. Listen to iframe scroll event
2. Debounce 50ms
3. Find topmost visible element with data-loc attribute
4. Parse data-loc to get line number
5. Calculate editor scroll position to show that line
6. editor.setScrollTop() without moving cursor
```

### Data Structure for Location Lookup

```typescript
interface SourceLocation {
  fileId: number;
  startLine: number;
  startCol: number;
  endLine: number;
  endCol: number;
}

// Parse data-loc attribute: "0:5:1-5:41"
function parseDataLoc(dataLoc: string): SourceLocation | null {
  const match = dataLoc.match(/^(\d+):(\d+):(\d+)-(\d+):(\d+)$/);
  if (!match) return null;
  return {
    fileId: parseInt(match[1]),
    startLine: parseInt(match[2]),
    startCol: parseInt(match[3]),
    endLine: parseInt(match[4]),
    endCol: parseInt(match[5]),
  };
}
```

### UI Component

```typescript
// In Editor.tsx toolbar area
<button
  onClick={() => setScrollSyncEnabled(!scrollSyncEnabled)}
  className={scrollSyncEnabled ? 'active' : ''}
  title="Toggle matched scrolling"
>
  <ScrollSyncIcon />
</button>
```

## Implementation Plan

### Phase 1: Configuration Infrastructure (k-ic1o)

See `claude-notes/plans/2025-12-29-config-integration-pipeline.md` for detailed tasks.

Summary:
- Define `ConfigValue` type in `quarto-core`
- Implement config merging in render pipeline
- Add `render_qmd_content_with_options()` to WASM
- Update TypeScript bindings

### Phase 2: Scroll Sync Implementation

**Task 2.1: Create useScrollSync hook**
- File: `hub-client/src/hooks/useScrollSync.ts` (new)
- Encapsulate all scroll synchronization logic
- Parameters: `editorRef`, `iframeRef`, `enabled`
- Returns: nothing (side-effect only hook)

**Task 2.2: Implement editor → preview sync**
- Listen to Monaco `onDidChangeCursorPosition`
- Build element lookup index on iframe load
- Query by line number, find best match
- Scroll with `scrollIntoView({ block: 'nearest' })`

**Task 2.3: Implement preview → editor sync**
- Listen to iframe scroll events
- Find topmost visible element with `data-loc`
- Calculate corresponding editor scroll position
- Call `editor.setScrollTop()`

**Task 2.4: Add debouncing**
- 50ms debounce for both directions
- Use `requestAnimationFrame` for smooth updates
- Cancel pending syncs when new events arrive

### Phase 3: UI Integration

**Task 3.1: Add scroll sync toggle**
- File: `hub-client/src/components/Editor.tsx`
- Add state: `scrollSyncEnabled`
- Add toggle button in toolbar
- Pass state to `useScrollSync` hook

**Task 3.2: Styling**
- File: `hub-client/src/components/Editor.css`
- Style for toggle button (active/inactive states)
- Consider icon for scroll sync

### Phase 4: Testing and Polish

**Task 4.1: Manual testing scenarios**
- Large documents with many headings
- Documents with code blocks
- Documents with images and figures
- Edge cases: empty document, very long lines

**Task 4.2: Performance verification**
- Profile with large documents
- Ensure no dropped frames during scroll
- Verify debouncing works correctly

**Task 4.3: Edge case handling**
- What if preview content doesn't have data-loc? (graceful degradation)
- What if line number not found? (no-op)
- What if iframe not loaded? (no-op)

## File Changes Summary

### New Files
- `hub-client/src/hooks/useScrollSync.ts`

### Modified Files
- `crates/wasm-quarto-hub-client/src/lib.rs` - New render function with options
- `crates/quarto-core/src/pipeline.rs` - Metadata injection (possibly new transform file)
- `hub-client/src/services/wasmRenderer.ts` - New TypeScript bindings
- `hub-client/src/components/Editor.tsx` - Toggle UI and hook integration
- `hub-client/src/components/Editor.css` - Toggle styling

## Open Questions

1. **Collaborative editing**: Should scroll sync work per-user or globally?
   - Recommendation: Per-user (local state only)

2. **Persistence**: Should scroll sync preference be saved?
   - Recommendation: Start with session-only, add persistence later if needed

3. **Multiple files**: When switching files, should scroll position reset?
   - Recommendation: Yes, reset to top

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Performance with large documents | Use efficient DOM queries, cache element index |
| iframe cross-origin issues | Already using `sandbox="allow-same-origin"` |
| Monaco API changes | Pin Monaco version, use stable APIs |
| Scroll jitter | Thorough debouncing, `requestAnimationFrame` |

## Success Criteria

1. Cursor movement in editor smoothly scrolls preview (when target not visible)
2. Scrolling preview smoothly scrolls editor viewport
3. No visible jitter or lag during scrolling
4. Toggle works correctly
5. Feature degrades gracefully when source locations unavailable

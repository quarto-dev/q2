# Scroll Sync / Image Layout Shift Bug

**Date**: 2026-01-10
**Status**: Analysis complete, ready for implementation

## Problem Statement

When editing a document with images in hub-client, the preview exhibits undesirable rescrolling behavior. This is caused by an interaction between the image data URI replacement system and the scroll sync feature.

## Root Cause Analysis

### The Image Processing Pipeline

1. **WASM Rendering** (`Editor.tsx:374-456`): Content is rendered to HTML with regular image paths (e.g., `![](images/foo.png)` → `<img src="/images/foo.png">`)

2. **Double-Buffered Swap** (`Editor.tsx:264-312`): HTML is loaded into the **inactive** iframe, CSS is processed (lines 280-294), then iframes are swapped

3. **Post-Processing on Active Iframe** (`Editor.tsx:315-318`): After swap, `handleActiveIframeLoad()` runs `postProcessIframe()` which replaces image `src` attributes with data URIs

4. **Layout Shift**: When data URI images decode and render, their actual dimensions cause DOM reflow on the **visible** iframe

### The Scroll Sync Mechanism

The scroll sync system (`useScrollSync.ts`) uses:
- `data-loc` attributes on HTML elements to map editor lines to preview elements
- `findElementForLine()` queries elements by line range
- `getBoundingClientRect()` to get element positions
- `scrollIntoView()` for editor→preview sync
- Scroll ratio for preview→editor sync

### The Conflict

```
Timeline:
[WASM render]
    ↓
[HTML into inactive iframe]
    ↓
[CSS processed on inactive iframe]
    ↓
[Swap: inactive → active]
    ↓
[handleActiveIframeLoad() fires]
    ↓
[postProcessIframe() replaces img.src with data URIs]  ← Images processed on VISIBLE iframe
    ↓
[Browser loads data URIs]
    ↓
[Images get actual dimensions]
    ↓
[LAYOUT SHIFT on visible content]  ← User sees jump
    ↓
[Scroll sync fires with new/changing positions]
    ↓
[Undesirable rescrolling]
```

The key issue: **Images are processed AFTER the iframe becomes visible**, causing visible layout shifts that trigger scroll sync with unstable positions.

## Key Code Locations

| File | Lines | Role |
|------|-------|------|
| `src/components/Editor.tsx` | 264-312 | Inactive iframe processing (CSS only!) |
| `src/components/Editor.tsx` | 315-318 | Active iframe load handler (triggers full post-process) |
| `src/utils/iframePostProcessor.ts` | 43-76 | Image src → data URI replacement |
| `src/hooks/useScrollSync.ts` | 117-143 | Editor→Preview sync using `scrollIntoView()` |
| `src/hooks/useScrollSync.ts` | 146-181 | Preview→Editor sync using scroll ratio |

## Proposed Solutions

### Option 1: Process Images on Inactive Iframe Before Swap (Recommended)

**Approach**: Extend `handleInactiveIframeLoad()` to process images (not just CSS) before the swap occurs.

**Implementation**:
1. Move image processing from `postProcessIframe()` to `handleInactiveIframeLoad()`
2. Wait for all image data URIs to be set before swapping
3. Optionally wait for image `onload` events to ensure layout is stable

**Pros**:
- Minimal architectural change
- Follows existing pattern (CSS is already processed before swap)
- Layout is stable when iframe becomes visible

**Cons**:
- Slightly delays content appearing (images must be processed first)
- Need to handle edge cases (missing images, errors)

### Option 2: Defer Scroll Sync Until Images Load

**Approach**: Track image load state and only enable scroll sync after all images have loaded.

**Implementation**:
1. After post-processing, attach `onload` handlers to all images
2. Track pending image count
3. Set a "layout stable" flag when all images load
4. Scroll sync checks this flag before operating

**Pros**:
- Works with any image source (not just data URIs)
- Robust against async image loading

**Cons**:
- More complex state management
- Scroll sync disabled during image loading (could be noticeable)

### Option 3: Reserve Space for Images

**Approach**: Set image dimensions in HTML before images load to prevent layout shift.

**Implementation**:
1. Store image dimensions in VFS metadata when uploading
2. WASM renderer emits `<img width="X" height="Y">` attributes
3. Layout is stable from the start

**Pros**:
- Best user experience (no layout shift ever)
- Works with scroll sync immediately

**Cons**:
- Requires schema change for binary files
- Need to compute/store dimensions on upload
- Doesn't help with external images

### Option 4: Lock Scroll During Post-Processing

**Approach**: Save scroll position before post-processing, restore after images load.

**Implementation**:
1. Save `scrollY` before `postProcessIframe()`
2. Attach image load handlers
3. After all images load, restore scroll position

**Pros**:
- Simple to implement
- No architectural changes

**Cons**:
- User sees scroll position jump (jarring)
- Doesn't prevent the underlying layout shift

## Recommended Implementation: Option 1

Process images on the inactive iframe before swapping. This is the most aligned with the current architecture and provides the best user experience.

### Implementation Steps

1. **Extract image processing from `postProcessIframe()`** into a separate function
2. **Update `handleInactiveIframeLoad()`** to call image processing in addition to CSS
3. **Ensure swap happens after images are processed** (already synchronous since VFS reads are sync)
4. **Test with various document types** (many images, large images, missing images)
5. **Consider optional image preload** - if images are large, might want to wait for decode

### Code Changes

```typescript
// In Editor.tsx handleInactiveIframeLoad:

// Existing: Process CSS (lines 284-293)
// ADD: Process images (similar to iframePostProcessor.ts lines 43-76)
doc.querySelectorAll('img').forEach((img) => {
  const src = img.getAttribute('src');
  if (!src || src.startsWith('http') || src.startsWith('data:')) return;

  // ... same logic as iframePostProcessor
  // Replace src with data URI
});

// Then swap happens with images already as data URIs
```

The key insight: since VFS reads are synchronous, image processing is also synchronous. We just need to do it in the right place (before swap, not after).

## Testing Plan

1. **Regression test**: Edit document with images, verify no rescrolling
2. **Image types**: Test with PNG, JPEG, GIF, SVG, WebP
3. **Edge cases**:
   - Missing image files
   - External URLs (should be skipped)
   - Very large images
   - Many images in one document
4. **Scroll sync verification**: Cursor movement correctly syncs with preview
5. **Performance**: Ensure no noticeable delay in content appearing

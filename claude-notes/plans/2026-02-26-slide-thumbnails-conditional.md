---
title: 'Conditional Slide Thumbnails in Outline Pane'
date: 2026-02-26
description: 'Limits outline pane slide thumbnails to slide-format documents by passing the detected format from the preview router to the editor, so thumbnails clear when a regular document is shown.'
---

## Overview

The slide thumbnail feature in the outline pane currently generates and displays thumbnails for ALL documents, not just slide-format documents (`format: q2-slides`). When switching from a slides document to a regular HTML document, thumbnails continue to appear in the outline. This plan fixes the issue by making thumbnail generation conditional on the document format.

## Root Cause Analysis

The data flow currently works like this:

1. `PreviewRouter.tsx` parses the AST and checks for `format: q2-slides` to route between `ReactPreview` (slides) and `Preview` (HTML).
2. `Editor.tsx` calls `useSlideThumbnails()` unconditionally with the AST JSON.
3. `useSlideThumbnails` calls `parseSlides()` which splits ANY document by h1/h2 headers — so it produces "slides" for regular documents too.
4. The generated thumbnails are always passed to `OutlinePanel`.

The format detection in `PreviewRouter` is isolated — it never communicates the detected format back to `Editor.tsx`. So `Editor.tsx` has no way to know whether thumbnails should be generated.

## Approach

Add an `onFormatChange` callback from `PreviewRouter` to `Editor`, so that `Editor` tracks whether the current document is slides format, and only generates/passes thumbnails when it is.

## Work Items

- [x] Add `onFormatChange` callback prop to `PreviewRouter`
- [x] Call `onFormatChange` from `PreviewRouter.checkFormat()` when format is determined
- [x] Add `isSlideFormat` state to `Editor.tsx`
- [x] Wire `onFormatChange` from `PreviewRouter` to update `isSlideFormat` in `Editor`
- [x] Pass `isSlideFormat` as `enabled` to `useSlideThumbnails` so it returns empty map when false
- [x] `useSlideThumbnails` clears thumbnails via useEffect when `enabled` becomes false
- [x] Remove debug `console.log('YOOOO', ...)` from `PreviewRouter.tsx`
- [x] TypeScript compiles cleanly (`npx tsc --noEmit`)
- [ ] Test: verify thumbnails appear for `.qmd` files with `format: q2-slides`
- [ ] Test: verify thumbnails do NOT appear for regular `.qmd` files
- [ ] Test: verify switching from slides to regular clears thumbnails in outline

## Design Notes

### Why `onFormatChange` callback instead of lifting format detection?

`PreviewRouter` already does the AST parse to decide which component to render. Lifting the detection to `Editor` would mean parsing twice or refactoring the preview routing significantly. A callback is minimal and keeps the existing architecture intact.

### Alternative considered: check format inside `useSlideThumbnails`

We could have `useSlideThumbnails` check the AST metadata for `format: q2-slides` itself. This is simpler but less correct — the hook shouldn't need to know about format semantics. It's cleaner for `Editor` to decide when to generate thumbnails based on format state.

### Clearing thumbnails on format change

When `isSlideFormat` transitions from true to false, `useSlideThumbnails` should return an empty `ThumbnailMap`. This ensures the outline pane immediately stops showing thumbnails.

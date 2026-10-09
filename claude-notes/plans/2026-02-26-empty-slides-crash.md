---
title: 'Fix: Slide Renderer Crashes on Empty Slides Document'
date: 2026-02-26
description: 'Fixes a slide renderer crash on documents with `format: q2-slides` but no slide content, by showing an empty slide area and hiding navigation controls rather than indexing an empty array.'
---

## Overview

When a document has `format: q2-slides` in YAML frontmatter but no slide content (no headers, no title), the slide renderer crashes. `parseSlides()` returns an empty array, and `SlideAst` tries to render `slides[0]` which is `undefined`, causing a crash on `slide.type` access at line 415 of `ReactAstSlideRenderer.tsx`.

## Root Cause

In `SlideAst` (ReactAstSlideRenderer.tsx:174):
```tsx
{renderSlide(slides[currentSlide], currentFilePath, onNavigateToDocument)}
```

When `slides` is empty, `slides[0]` is `undefined`. The `renderSlide` function immediately accesses `slide.type` (line 415), which throws.

The existing clamp logic (line 129) doesn't help because it guards with `slides.length > 0`, so it does nothing when the array is empty.

## Approach

Handle the empty slides case in `SlideAst` by rendering an empty placeholder slide when `slides.length === 0`. This is the simplest fix — a single early return before the main render. The navigation controls and slide counter should also be hidden since there's nothing to navigate.

## Work Items

- [x] Guard `renderSlide` call in `SlideAst` JSX: only render when `slides.length > 0`
- [x] Hide navigation buttons when `slides.length <= 1`
- [x] Hide slide counter when `slides.length === 0`
- [x] TypeScript compiles cleanly (`npx tsc --noEmit`)
- [ ] Test: frontmatter with only `format: q2-slides` — should show empty white slide, no crash
- [ ] Test: frontmatter with `title:` and `format: q2-slides` — title slide renders normally
- [ ] Test: adding headers after empty state — slides appear as content is typed

## Design Notes

### Why an early return instead of a sentinel slide?

Adding a sentinel/placeholder slide to `parseSlides` would pollute the slide array for all consumers (including thumbnail generation). It's cleaner to handle "no slides" at the component level. The empty state is a UI concern, not a parsing concern.

### What should the empty state look like?

A white slide area (matching the normal slide background) with no navigation controls. This gives a clean starting point and naturally transitions to showing content once the user types headers. No "empty state" text is needed — the user sees the format is working and just needs to add content.

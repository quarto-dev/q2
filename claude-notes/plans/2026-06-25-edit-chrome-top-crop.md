---
title: 'Fix: edit chrome cropped at the top of the viewport'
date: 2026-06-25
---

**Strand:** bd-pvcnea83
**Date:** 2026-06-25
**Status:** in progress

## Problem (confirmed in the binary)

Editing the first block of a title-less document (or any block scrolled near the
viewport top) crops the floating edit chrome. The rich-text toolbar is
`position:absolute; bottom:100%; margin-bottom:4px` (floats *above* the edit
box); for a block flush against the top of the scroll area it lands at negative
`top` (above the iframe viewport, `scrollTop` already 0) and is clipped — only a
sliver shows. Measured: editor `top:15`, toolbar `top:-15.4 → bottom:11`
(height 26.4). The inline nesting breadcrumb lives in the toolbar, so it crops
too. The standalone `BreadcrumbChip` (non-rich blocks at the top) has the same
top-crop: its geometry sets `top = surfaceTop − chipH`, negative for a top
surface.

## Approach: collision-aware flip (above → below)

Standard floating-toolbar behavior. When there isn't room above
(`surfaceTop − chromeHeight − gap < 0`, viewport-relative), render the chrome
**below** the block instead of above. While editing a top block the chrome then
sits just under it (briefly over the next block), fully visible; the edited text
is never covered. Parity-neutral (no change to the rendered/preview document
spacing). Generalizes correctly to any near-top block, not just the literal
first one.

Shared pure helper (mirrors the chip's existing `computeChipGeometry` pattern):

```ts
// editChromeGeometry.ts
export function shouldPlaceChromeBelow(surfaceTop: number, chromeHeight: number, gap: number): boolean {
  return surfaceTop - chromeHeight - gap < 0;
}
```

- **Toolbar** (`RichTextToolbar`): `useLayoutEffect` measures the
  `.q2-richtext-editor` box's viewport top + the toolbar's height; if it would
  clip above, set a `q2-rt-toolbar-below` class (`top:100%; bottom:auto;
  margin-top:4px`). Guard: skip when `offsetHeight <= 0` (degenerate jsdom rects)
  so the default `above` placement is kept there.
- **Chip** (`BreadcrumbChip`): in the geometry effect, when
  `shouldPlaceChromeBelow(sRect.top, chipH, GAP)` (and real geometry —
  `sRect.height > 0`), set `top = (sRect.bottom − hostRect.top) + GAP` (below)
  instead of `surfaceTop − chipH` (above). Horizontal left-spill geometry is
  unchanged.

Both compute placement from the *surface's* stable top (not the chrome's flipped
position), so there is no flip/re-measure loop.

## Tests (TDD)

1. **Unit (RED first):** `editChromeGeometry.test.ts` — `shouldPlaceChromeBelow`
   true for `(15, 26, 4)` (the measured case), false for `(100, 26, 4)`, and
   boundary cases.
2. **Real-binary e2e:** title-less fixture; edit the first block →
   - rich Para: `.q2-rt-toolbar` has `q2-rt-toolbar-below` and its rect top ≥ 0
     (not clipped);
   - first block is a code block (non-rich): standalone chip rect top ≥ 0
     (placed below).
   Follow the established floating-chrome testing pattern (pure unit + real e2e;
   jsdom rects are degenerate, so vertical placement is not asserted in jsdom).

## Notes / known interactions

- The hub-client `q2-preview-breadcrumb-geometry` e2e asserts "chip above
  surface" for a TOP paragraph. That suite is already red (bd-fpys25b0,
  rich-text default-on) and will need its top-block expectation updated to the
  flipped (below) placement when bd-fpys25b0 is addressed. Out of scope here;
  noted on that strand.

## Work items

- [x] `editChromeGeometry.ts` + unit test (`shouldPlaceChromeBelow`, 5 cases)
- [x] Toolbar: `useLayoutEffect` measure + `q2-rt-toolbar-below` class + CSS
- [x] Chip: flip `top` below when clipped (guarded on `sRect.height > 0`)
- [x] preview-renderer unit (505) + integration (515) suites green
- [x] Real-binary e2e on title-less fixtures (toolbar + chip),
      `q2-preview-spa/e2e/edit-chrome-placement.spec.ts` (2 tests); full spa e2e
      suite green (39). Verified live in Chrome: toolbar flips below (top 48.5,
      uncropped) for the first paragraph; chip flips below (top 83.8, uncropped)
      for a first code block.
- [x] hub-client build + unit (662) + integration (76) green;
      `cargo xtask verify --skip-hub-build` green (one flaky pampa-oracle spike
      failure on first run, passed on re-run — unrelated to this change)
- [ ] hub-client/changelog.md (two-commit) — pending commit

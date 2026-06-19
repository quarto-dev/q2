/**
 * Tests for `computeChipGeometry` — the pure breadcrumb-chip layout math
 * (G1 left-spill). Extracted from `BreadcrumbChip`'s `useLayoutEffect` so the
 * geometry is unit-testable without jsdom layout.
 *
 * These tests pin the STRUCTURAL relationships of the left-spill model, NOT the
 * `CRUMB_W` pixel value — every expectation derives `naturalWidth` from the
 * exported `CRUMB_W` constant, so live-tuning `CRUMB_W` cannot break them.
 *
 * Pure function, node env (no DOM).
 */

import { describe, it, expect } from 'vitest';
import { computeChipGeometry, CRUMB_W, MIN_GLYPH_W } from './BreadcrumbChip';

// ◀ button width — mirrors the module's `OUT_W = MIN_GLYPH_W`.
const OUT_W = MIN_GLYPH_W;

describe('computeChipGeometry (G1 left-spill)', () => {
  it('(a) deep indent that already fits the gutter is unchanged (regression pin)', () => {
    // gutter (100) ≥ naturalWidth (2·CRUMB_W) → no spill: band fills the gutter,
    // chip anchored at colLeft − ◀ exactly like the old gutter-only model.
    const colLeft = 200;
    const surfaceLeft = 300; // gutter = 100
    const g = computeChipGeometry(surfaceLeft, colLeft, 2);

    expect(surfaceLeft - colLeft).toBeGreaterThanOrEqual(2 * CRUMB_W); // precondition
    expect(g.bandWidth).toBe(surfaceLeft - colLeft); // band == gutter
    expect(g.chipLeft).toBe(colLeft - OUT_W); // old anchor preserved
    expect(g.slots).toBeGreaterThanOrEqual(2); // full path
  });

  it('(b) shallow indent (blockquote, 2 crumbs) spills LEFT and keeps both crumbs', () => {
    // gutter (24) < naturalWidth (2·CRUMB_W ≥ 32) → band grows to the comfortable
    // natural width and the chip spills left into the margin. THIS IS THE G1 BUG FIX.
    const colLeft = 100;
    const surfaceLeft = 124; // gutter = 24
    const g = computeChipGeometry(surfaceLeft, colLeft, 2);

    expect(surfaceLeft - colLeft).toBeLessThan(2 * CRUMB_W); // shallow precondition
    expect(g.bandWidth).toBe(2 * CRUMB_W); // band == naturalWidth (comfortable)
    // Right edge still pinned at the pivot: chipLeft = surfaceLeft − ◀ − band.
    expect(g.chipLeft).toBe(surfaceLeft - OUT_W - 2 * CRUMB_W);
    expect(g.chipLeft).toBeLessThan(colLeft - OUT_W); // spilled left past the old anchor
    expect(g.slots).toBeGreaterThanOrEqual(2); // both crumbs survive
  });

  it('(c) out of room on the left: ◀ clamps to x=0 and the band spills RIGHT past the pivot', () => {
    // Small surfaceLeft + long path: the comfortable band cannot fit to the left of
    // the pivot. Rather than crunch/ellipsize, pin ◀ at the page edge and let the
    // band extend RIGHT past the pivot (over the content) — the full path stays
    // visible. The content column has room, so this adds no horizontal scrollbar.
    const colLeft = 20;
    const surfaceLeft = 40;
    const g = computeChipGeometry(surfaceLeft, colLeft, 5); // naturalWidth = 5·CRUMB_W ≫ left space

    expect(g.chipLeft).toBe(0); // ◀ clamped to the page edge
    expect(g.bandWidth).toBe(5 * CRUMB_W); // comfortable width kept; band spills right
    expect(g.slots).toBeGreaterThanOrEqual(5); // full path, no ellipsize
  });

  it('(d) unmeasured layout (jsdom, surfaceLeft ≤ 0) shows the full path', () => {
    const g = computeChipGeometry(0, 0, 3);
    expect(g.slots).toBe(3); // == crumbCount: never collapse when we cannot measure
  });
});

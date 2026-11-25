import { describe, expect, it } from 'vitest';
import { scrollDelta, wheelBursts } from '../../src/scroll';

// A pane 800 px tall starting at y = 100, rows 20 px high.
const pane = { y: 100, height: 800 };
const row = (y: number) => ({ y, height: 20 });

describe('scrollDelta: how far to wheel to bring a row into view (0 means leave the editor alone)', () => {
  it('leaves a row alone while the text it leads into still fits below it (twelve rows of room)', () => {
    expect(scrollDelta(row(300), pane)).toBe(0);
    expect(scrollDelta(row(130), pane)).toBe(0);
    expect(scrollDelta(row(100), pane)).toBe(0);
    expect(scrollDelta(row(660), pane)).toBe(0); // 660 + 12 rows of 20 px = 900, the bottom of the pane
  });

  it('scrolls down when the row is below the pane, or so low that the text after it would be cut off', () => {
    expect(scrollDelta(row(1500), pane)).toBeGreaterThan(0);
    expect(scrollDelta(row(880), pane)).toBeGreaterThan(0);
    expect(scrollDelta(row(700), pane)).toBeGreaterThan(0);
  });

  it('scrolls up when the row is above the pane', () => {
    expect(scrollDelta(row(-400), pane)).toBeLessThan(0);
    expect(scrollDelta(row(60), pane)).toBeLessThan(0);
  });

  it('aims the row at about a fifth of the way down the pane', () => {
    const d = scrollDelta(row(1500), pane);
    expect(1500 - d).toBeCloseTo(100 + 800 * 0.2, 0);
  });

  it('works in a short pane too: near the top there is room, lower down it scrolls', () => {
    const short = { y: 100, height: 300 }; // twelve 20 px rows need 240 px
    expect(scrollDelta(row(110), short)).toBe(0);
    expect(scrollDelta(row(250), short)).toBeGreaterThan(0);
  });

  it('does nothing when the row is not rendered, so there is nothing to measure', () => {
    expect(scrollDelta(null, pane)).toBe(0);
  });

  it('converges: after scrolling by the delta the same row needs no further move', () => {
    for (const y of [1500, 880, 700, 60, -400, 5000]) {
      const d = scrollDelta(row(y), pane);
      expect(scrollDelta(row(y - d), pane)).toBe(0);
    }
  });
});

describe('wheelBursts: a long scroll as one quick burst of small wheel events', () => {
  it('splits a distance into steps no larger than the limit, summing to the distance', () => {
    const steps = wheelBursts(683);
    expect(steps.every((x) => Math.abs(x) <= 40)).toBe(true);
    expect(steps.reduce((a, b) => a + b, 0)).toBe(683);
  });

  it('keeps the sign, so scrolling up is a burst of negative steps', () => {
    const steps = wheelBursts(-130);
    expect(steps.every((x) => x < 0)).toBe(true);
    expect(steps.reduce((a, b) => a + b, 0)).toBe(-130);
  });

  it('is empty for no distance', () => {
    expect(wheelBursts(0)).toEqual([]);
  });
});

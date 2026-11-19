import { describe, expect, it } from 'vitest';
import { createTypist } from '../../src/typing';

const TEXT = 'git switch feature, then look at the abstract. It reads differently now; Myra rewrote it: carefully.';

describe('typist', () => {
  it('repeats for one seed and differs between seeds', () => {
    const a = createTypist({ baseMs: 40, seed: 7 }).plan(TEXT);
    const b = createTypist({ baseMs: 40, seed: 7 }).plan(TEXT);
    const c = createTypist({ baseMs: 40, seed: 8 }).plan(TEXT);
    expect(a).toEqual(b);
    expect(a.gaps).not.toEqual(c.gaps);
  });

  it('keeps every delay within bounds', () => {
    const base = 40;
    const p = createTypist({ baseMs: base, seed: 1 }).plan(TEXT);
    expect(p.gaps).toHaveLength(TEXT.length);
    expect(p.gaps[0]).toBe(0); // the lead pause is separate
    for (const g of p.gaps.slice(1)) {
      expect(g).toBeGreaterThanOrEqual(base * 0.5);
      expect(g).toBeLessThanOrEqual(base * 1.8 * 2 + 400);
    }
    expect(p.lead).toBeGreaterThanOrEqual(250);
    expect(p.lead).toBeLessThanOrEqual(600);
    expect(p.beforeEnter).toBeGreaterThanOrEqual(300);
    expect(p.beforeEnter).toBeLessThanOrEqual(700);
  });

  it('waits longer after spaces and punctuation on average, and sometimes hesitates', () => {
    const long = (TEXT + ' ').repeat(40);
    const p = createTypist({ baseMs: 40, seed: 3 }).plan(long);
    const mean = (xs: number[]) => xs.reduce((s, x) => s + x, 0) / xs.length;
    const after: number[] = [];
    const other: number[] = [];
    for (let i = 1; i < long.length; i++) (' ,.;:'.includes(long[i - 1]!) ? after : other).push(p.gaps[i]!);
    expect(mean(after)).toBeGreaterThan(mean(other));
    expect(p.gaps.filter((g) => g >= 150).length).toBeGreaterThan(0);
    expect(p.gaps.filter((g) => g >= 150).length).toBeLessThan(long.length / 5);
  });

  it('is exactly zero everywhere when the base is zero (fast mode)', () => {
    const p = createTypist({ baseMs: 0, seed: 9 }).plan(TEXT);
    expect(p).toEqual({ lead: 0, gaps: new Array(TEXT.length).fill(0), beforeEnter: 0 });
  });

  it('advances its generator across calls, reproducibly', () => {
    const t1 = createTypist({ baseMs: 40, seed: 5 });
    const t2 = createTypist({ baseMs: 40, seed: 5 });
    const first = t1.plan('abc def');
    const second = t1.plan('abc def');
    expect(second).not.toEqual(first);
    expect(t2.plan('abc def')).toEqual(first);
    expect(t2.plan('abc def')).toEqual(second);
  });
});

/**
 * Human typing tempo for the scripted demo. One seeded generator plans the delays of every typed text, so a recorded
 * run is reproducible (`--seed`) and a different seed gives a different rhythm. A base of 0 (`--fast`) plans all zeros.
 */

export interface TypingPlan {
  /** The pause before the first character, in ms. */
  lead: number;
  /** The wait before character `i`, in ms. `gaps[0]` is 0: the lead pause covers it. */
  gaps: number[];
  /** The pause between the last character and Enter, in ms. */
  beforeEnter: number;
}

export interface Typist {
  plan(text: string): TypingPlan;
}

/** mulberry32: a small seeded generator returning floats in [0, 1). */
function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function createTypist(o: { baseMs: number; seed: number }): Typist {
  const next = rng(o.seed);
  const between = (lo: number, hi: number) => lo + (hi - lo) * next();
  return {
    plan(text) {
      const chars = [...text];
      if (o.baseMs <= 0) return { lead: 0, gaps: chars.map(() => 0), beforeEnter: 0 };
      const lead = between(250, 600);
      const gaps = chars.map((_, i) => {
        if (i === 0) return 0;
        // A base wobble of 0.5 to 1.8 times the base; slower after a space (x1.6) and after punctuation (x2).
        let g = o.baseMs * between(0.5, 1.8);
        const prev = chars[i - 1]!;
        if (prev === ' ') g *= 1.6;
        else if (',.;:!?'.includes(prev)) g *= 2;
        // An occasional hesitation of 150 to 400 ms.
        if (next() < 0.04) g += between(150, 400);
        return g;
      });
      return { lead, gaps, beforeEnter: between(300, 700) };
    },
  };
}

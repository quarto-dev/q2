import { describe, expect, it } from 'vitest';
import { applySteps, createTypist, type TypingPlan } from '../../src/typing';

const TEXT = 'git switch feature, then look at the abstract. It reads differently now; Myra rewrote it: carefully.';
const LONG_TOKEN = 'ws://127.0.0.1:51391/project/4UcGpVm61Gzf7bWevYMkL1abffhE';
const mean = (xs: number[]) => xs.reduce((s, x) => s + x, 0) / xs.length;

/** The typed steps (single characters), each with the text produced before it, for statistics on what precedes a key. */
function typedWithPrev(plan: TypingPlan): { ch: string; prev: string; wait: number }[] {
  const out: { ch: string; prev: string; wait: number }[] = [];
  let sofar = '';
  for (const s of plan.steps) {
    if (s.text !== undefined && !s.paste) out.push({ ch: s.text, prev: sofar.slice(-1), wait: s.wait });
    sofar = applySteps([...plan.steps.slice(0, plan.steps.indexOf(s) + 1)]);
  }
  return out;
}

describe('typist: basics', () => {
  it('repeats for one seed and differs between seeds', () => {
    const a = createTypist({ baseMs: 40, seed: 7 }).plan(TEXT);
    const b = createTypist({ baseMs: 40, seed: 7 }).plan(TEXT);
    const c = createTypist({ baseMs: 40, seed: 8 }).plan(TEXT);
    expect(a).toEqual(b);
    expect(a.steps).not.toEqual(c.steps);
  });

  it('always produces exactly the input text, whatever the seed, mode, and typo rate', () => {
    for (let seed = 0; seed < 60; seed++) {
      for (const paste of ['auto', 'all', 'none'] as const) {
        for (const typoRate of [0, 0.006, 0.5]) {
          const text = `${TEXT} --project ${LONG_TOKEN} Done.`;
          const plan = createTypist({ baseMs: 40, seed, typoRate }).plan(text, { paste });
          expect(applySteps(plan.steps)).toBe(text);
        }
      }
    }
  });

  it('keeps the lead and final pauses and every wait within bounds', () => {
    for (let seed = 0; seed < 30; seed++) {
      const p = createTypist({ baseMs: 40, seed }).plan(`${TEXT} ${LONG_TOKEN}`);
      expect(p.lead).toBeGreaterThanOrEqual(250);
      expect(p.lead).toBeLessThanOrEqual(600);
      expect(p.beforeEnter).toBeGreaterThanOrEqual(300);
      expect(p.beforeEnter).toBeLessThanOrEqual(700);
      for (const s of p.steps) {
        expect(s.wait).toBeGreaterThanOrEqual(0);
        expect(s.wait).toBeLessThanOrEqual(2500);
      }
    }
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

  it('is exactly zero, one step per character, with no typos or pastes, when the base is zero (fast mode)', () => {
    for (const paste of ['auto', 'all'] as const) {
      const text = `${TEXT} ${LONG_TOKEN}`;
      const p = createTypist({ baseMs: 0, seed: 9, typoRate: 1 }).plan(text, { paste });
      expect(p.lead).toBe(0);
      expect(p.beforeEnter).toBe(0);
      expect(p.steps).toEqual([...text].map((c) => ({ wait: 0, text: c })));
    }
  });
});

describe('typist: how people type', () => {
  const noTypos = { baseMs: 40, typoRate: 0 };

  it('pauses at the start of a word more than inside one', () => {
    const t = createTypist({ ...noTypos, seed: 3 });
    const words = ('the quick brown fox jumps over the lazy dog and then the dog sleeps while the fox runs away ').repeat(8);
    const typed = typedWithPrev(t.plan(words, { paste: 'none' }));
    const starts = typed.filter((k) => k.prev === ' ' && /[a-z]/.test(k.ch)).map((k) => k.wait);
    const inside = typed.filter((k) => /[a-z]/.test(k.prev) && /[a-z]/.test(k.ch)).map((k) => k.wait);
    expect(mean(starts)).toBeGreaterThan(mean(inside) * 1.6);
  });

  it('is slower on capitals, digits, and symbols than on lowercase letters', () => {
    const t = createTypist({ ...noTypos, seed: 4 });
    const typed = typedWithPrev(t.plan('abcde Xyz-9_q &|~ ZZtop /Foo= bar'.repeat(30), { paste: 'none' }));
    const hard = typed.filter((k) => /[A-Z0-9\-_&|~/=]/.test(k.ch) && /[a-z]/.test(k.prev)).map((k) => k.wait);
    const easy = typed.filter((k) => /[a-z]/.test(k.ch) && /[a-z]/.test(k.prev)).map((k) => k.wait);
    expect(mean(hard)).toBeGreaterThan(mean(easy) * 1.25);
  });

  it('types alternating-hand pairs faster than same-hand pairs, and common digraphs fastest', () => {
    const t = createTypist({ ...noTypos, seed: 6 });
    const text = ('quality dazzled the junk of vexed brown foxes while kids jumped over a lazy dog and ').repeat(40);
    const typed = typedWithPrev(t.plan(text, { paste: 'none' }));
    const left = new Set('qwertasdfgzxcvb');
    const letters = typed.filter((k) => /[a-z]/.test(k.ch) && /[a-z]/.test(k.prev));
    const alt = letters.filter((k) => left.has(k.ch) !== left.has(k.prev)).map((k) => k.wait);
    const same = letters.filter((k) => left.has(k.ch) === left.has(k.prev)).map((k) => k.wait);
    expect(mean(alt)).toBeLessThan(mean(same));
    const common = new Set(['th', 'he', 'in', 'er', 'an', 're', 'on', 'at', 'en', 'nd']);
    const fast = letters.filter((k) => common.has(k.prev + k.ch)).map((k) => k.wait);
    expect(mean(fast)).toBeLessThan(mean(letters.map((k) => k.wait)));
  });

  it('sometimes hesitates, but not often', () => {
    const t = createTypist({ ...noTypos, seed: 3 });
    const typed = typedWithPrev(t.plan(('mmm nnn ').repeat(200), { paste: 'none' }));
    const slow = typed.filter((k) => /[a-z]/.test(k.prev) && /[a-z]/.test(k.ch) && k.wait >= 190);
    expect(slow.length).toBeGreaterThan(0);
    expect(slow.length).toBeLessThan(typed.length / 10);
  });
});

describe('typist: pasting', () => {
  it('pastes a long connect string in one step after a reaching pause, and types the rest', () => {
    const text = `q2 preview --hub-url ${LONG_TOKEN} --ui editor`;
    const p = createTypist({ baseMs: 40, seed: 2, typoRate: 0 }).plan(text);
    const pastes = p.steps.filter((s) => s.paste);
    expect(pastes.map((s) => s.text)).toEqual([LONG_TOKEN]);
    expect(pastes[0]!.wait).toBeGreaterThanOrEqual(400);
    expect(pastes[0]!.wait).toBeLessThanOrEqual(900);
    expect(p.steps.filter((s) => !s.paste).every((s) => [...(s.text ?? '')].length === 1)).toBe(true);
  });

  it('never pastes short words, and never when asked not to', () => {
    const p = createTypist({ baseMs: 40, seed: 2 }).plan(TEXT);
    expect(p.steps.some((s) => s.paste)).toBe(false);
    const q = createTypist({ baseMs: 40, seed: 2 }).plan(`x ${LONG_TOKEN}`, { paste: 'none' });
    expect(q.steps.some((s) => s.paste)).toBe(false);
  });

  it('pastes the whole text as one step when told it was copied', () => {
    const text = `q2 preview --hub-url ${LONG_TOKEN} --project abc`;
    const p = createTypist({ baseMs: 40, seed: 2 }).plan(text, { paste: 'all' });
    expect(p.steps).toHaveLength(1);
    expect(p.steps[0]).toMatchObject({ text, paste: true });
    expect(p.steps[0]!.wait).toBeGreaterThanOrEqual(400);
    expect(p.lead).toBeGreaterThanOrEqual(250);
  });
});

describe('typist: typos', () => {
  it('makes a typo in roughly one text in five or fewer, fixes it, and never in a paste or the first character', () => {
    let withTypo = 0;
    const N = 300;
    for (let seed = 0; seed < N; seed++) {
      const p = createTypist({ baseMs: 40, seed }).plan('git switch feature then look at the abstract carefully');
      if (p.steps.some((s) => s.backspace)) withTypo++;
      expect(applySteps(p.steps)).toBe('git switch feature then look at the abstract carefully');
      expect(p.steps[0]!.backspace).toBeUndefined();
    }
    expect(withTypo).toBeGreaterThan(N * 0.05);
    expect(withTypo).toBeLessThan(N * 0.45);
  });

  it('notices after a pause, deletes what it typed since the slip, and retypes it', () => {
    const p = createTypist({ baseMs: 40, seed: 1, typoRate: 1 }).plan('abcdef ghijkl');
    const i = p.steps.findIndex((s) => s.backspace);
    expect(i).toBeGreaterThan(0);
    expect(p.steps[i]!.wait).toBeGreaterThanOrEqual(250); // the first backspace comes after noticing
    const backs = p.steps.filter((s) => s.backspace).length;
    expect(backs).toBeGreaterThanOrEqual(1);
    expect(applySteps(p.steps)).toBe('abcdef ghijkl');
  });

  it('makes none at a rate of zero, and none inside a pasted token', () => {
    for (let seed = 0; seed < 40; seed++) {
      expect(createTypist({ baseMs: 40, seed, typoRate: 0 }).plan(TEXT).steps.some((s) => s.backspace)).toBe(false);
      const p = createTypist({ baseMs: 40, seed, typoRate: 1 }).plan(`go ${LONG_TOKEN}`, { paste: 'auto' });
      const pasteAt = p.steps.findIndex((s) => s.paste);
      expect(p.steps.slice(pasteAt).some((s) => s.backspace)).toBe(false);
    }
  });
});

/**
 * Human typing for the scripted demo. One seeded generator plans every typed text as a list of steps (type a
 * character, press Backspace, paste a chunk), so a recorded run is reproducible (`--seed`) and a different seed gives a
 * different rhythm. A base of 0 (`--fast`) plans one zero-wait step per character: no typos, no pastes.
 *
 * What it models, from how people type:
 * - **Thinking at word starts.** The first letter of a word waits longer than the rest, longer still after `&&`, `|`, or
 *   `;` (a new command), and now and then a person stops to think.
 * - **The keyboard.** Keys on alternating hands come faster than keys on one hand, a second press by the same finger is
 *   slow, common letter pairs ("th", "er", "in") are fast, and capitals, digits, and symbols cost extra (shift, reach).
 * - **Drift.** Speed wanders from word to word (bursts and lulls), and no keystroke gap is uniform: a gap is the sum of
 *   two draws, so it clusters around the typical value.
 * - **Pasting.** A long token (a URL, a project id, a path) is not typed: the person reaches for the clipboard (a pause)
 *   and it appears at once. A text can also be marked as copied as a whole.
 * - **Typos.** Very occasionally (at most one per text) a letter lands on a neighboring key; the person types on for a
 *   letter or two, notices after a pause, backspaces to the slip, and retypes. The final text is always the input.
 */

export type PasteMode = 'auto' | 'all' | 'none';

export interface TypingStep {
  /** The wait before this step, in ms. */
  wait: number;
  /** One typed character, or the whole chunk when `paste` is set. */
  text?: string;
  backspace?: true;
  paste?: true;
}

export interface TypingPlan {
  /** The pause before the first step, in ms. */
  lead: number;
  steps: TypingStep[];
  /** The pause between the last step and Enter, in ms. */
  beforeEnter: number;
}

export interface Typist {
  plan(text: string, o?: { paste?: PasteMode }): TypingPlan;
}

/** The text the steps leave behind. */
export function applySteps(steps: TypingStep[]): string {
  const out: string[] = [];
  for (const s of steps) {
    if (s.backspace) out.pop();
    else if (s.text !== undefined) out.push(...s.text);
  }
  return out.join('');
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

const ROWS = ['qwertyuiop', 'asdfghjkl', 'zxcvbnm'];
const LEFT = new Set('qwertasdfgzxcvb');
/** The finger of each letter on a QWERTY keyboard (0 to 3 left pinky to index, 4 to 7 right index to pinky). */
const FINGER: Record<string, number> = {};
for (const [f, keys] of Object.entries({ 0: 'qaz', 1: 'wsx', 2: 'edc', 3: 'rfvtgb', 4: 'yhnujm', 5: 'ik', 6: 'ol', 7: 'p' })) {
  for (const k of keys) FINGER[k] = Number(f);
}
const COMMON = new Set(
  'th he in er an re on at en nd ti es or te of ed is it al ar st to nt ng se ha as ou io le ve co me de hi ri ro ic ne ea ra ce li ch ll be ma si om ur'.split(' '),
);

/** The keys a finger can slip onto: the neighbors in the row and the staggered keys above and below. */
const NEIGHBORS: Record<string, string[]> = {};
ROWS.forEach((row, r) => {
  [...row].forEach((k, c) => {
    const n: (string | undefined)[] = [row[c - 1], row[c + 1], ROWS[r - 1]?.[c], ROWS[r - 1]?.[c + 1], ROWS[r + 1]?.[c - 1], ROWS[r + 1]?.[c]];
    NEIGHBORS[k] = n.filter((x): x is string => !!x);
  });
});

const isLetter = (c: string | undefined): c is string => !!c && /[A-Za-z]/.test(c);
const isLower = (c: string | undefined): c is string => !!c && /[a-z]/.test(c);
const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));

export function createTypist(o: { baseMs: number; seed: number; typoRate?: number }): Typist {
  const next = rng(o.seed);
  const between = (lo: number, hi: number) => lo + (hi - lo) * next();
  const typoRate = o.typoRate ?? 0.006;
  let tempo = 1;

  /** The wait before typing `c` after `prev` (`prev` is undefined at the start of the text). */
  const gap = (prev: string | undefined, c: string): number => {
    let f = 0.5 + 1.2 * ((next() + next()) / 2) * 1.0; // 0.5 to 1.7, clustered near 1.1
    const wordStart = prev === undefined || prev === ' ';
    if (wordStart) {
      tempo = 0.7 * tempo + 0.3 * between(0.8, 1.25); // a new word may be a burst or a lull
      f *= between(1.6, 3.2);
    } else if (isLower(prev) && isLower(c)) {
      const a = prev.toLowerCase();
      const b = c.toLowerCase();
      if (LEFT.has(a) !== LEFT.has(b)) f *= 0.88; // the other hand is already moving
      else if (FINGER[a] === FINGER[b] && a !== b) f *= 1.25; // the same finger has to move again
      if (COMMON.has(a + b)) f *= 0.75;
    } else if (',.;:!?'.includes(prev)) f *= 1.5;
    if (/[A-Z0-9]/.test(c) || (/[^A-Za-z0-9 ]/.test(c) && !wordStart)) f *= 1.5; // shift or a reach
    let ms = o.baseMs * tempo * f;
    if (wordStart && next() < 0.12) ms += between(200, 500); // stops to think
    if (next() < 0.025) ms += between(150, 400); // a hesitation anywhere
    return clamp(ms, 15, 2400);
  };

  return {
    plan(text, opts = {}) {
      const chars = [...text];
      if (o.baseMs <= 0) return { lead: 0, steps: chars.map((c) => ({ wait: 0, text: c })), beforeEnter: 0 };
      const mode = opts.paste ?? 'auto';
      const lead = between(250, 600);
      if (mode === 'all') {
        return { lead, steps: [{ wait: between(400, 900), text, paste: true }], beforeEnter: between(300, 700) };
      }

      // Pasted spans: tokens of 24 or more non-space characters.
      const pasted = new Array<boolean>(chars.length).fill(false);
      if (mode === 'auto') {
        for (let i = 0; i < chars.length; ) {
          if (chars[i] === ' ') {
            i++;
            continue;
          }
          let j = i;
          while (j < chars.length && chars[j] !== ' ') j++;
          if (j - i >= 24) for (let k = i; k < j; k++) pasted[k] = true;
          i = j;
        }
      }

      const steps: TypingStep[] = [];
      let typoUsed = false;
      let i = 0;
      while (i < chars.length) {
        const c = chars[i]!;
        if (pasted[i]) {
          let j = i;
          while (j < chars.length && pasted[j]) j++;
          steps.push({ wait: between(400, 900), text: chars.slice(i, j).join(''), paste: true });
          i = j;
          continue;
        }
        const prev = chars[i - 1];
        const eligible = !typoUsed && i > 0 && isLetter(c) && isLetter(prev) && NEIGHBORS[c.toLowerCase()] !== undefined;
        if (eligible && next() < typoRate) {
          typoUsed = true;
          // How many correct letters follow before the slip is noticed (none past the end of the word).
          let room = 0;
          while (isLetter(chars[i + 1 + room]) && !pasted[i + 1 + room]) room++;
          const r = next();
          const k = Math.min(room, r < 0.5 ? 0 : r < 0.8 ? 1 : 2);
          const near = NEIGHBORS[c.toLowerCase()]!;
          const slip = near[Math.floor(next() * near.length)]!;
          steps.push({ wait: gap(prev, c), text: c === c.toUpperCase() ? slip.toUpperCase() : slip });
          for (let n = 1; n <= k; n++) steps.push({ wait: gap(chars[i + n - 1], chars[i + n]!), text: chars[i + n]! });
          steps.push({ wait: between(250, 600), backspace: true });
          for (let n = 0; n < k; n++) steps.push({ wait: between(60, 120), backspace: true });
          steps.push({ wait: between(80, 160), text: c });
          for (let n = 1; n <= k; n++) steps.push({ wait: gap(chars[i + n - 1], chars[i + n]!), text: chars[i + n]! });
          i += k + 1;
          continue;
        }
        steps.push({ wait: gap(prev, c), text: c });
        i++;
      }
      return { lead, steps, beforeEnter: between(300, 700) };
    },
  };
}

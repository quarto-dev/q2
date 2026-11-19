import { describe, expect, it } from 'vitest';
import { routeMessage, StripBuffer, ITALIC_DIM, RED_ITALIC, RESET } from '../../src/captions';

const modes = ['inline', 'strip', 'both', 'off'] as const;

describe('caption routing (DM3)', () => {
  it.each(modes)('captions with --captions=%s', (m) => {
    const r = routeMessage({ kind: 'caption', text: 'hello' }, { captions: m, narration: 'off' });
    expect(r.inline !== undefined).toBe(m === 'inline' || m === 'both');
    expect(r.strip !== undefined).toBe(m === 'strip' || m === 'both');
  });

  it.each(modes)('narration with --narration=%s', (m) => {
    const r = routeMessage({ kind: 'narration', text: 'swapped' }, { captions: 'off', narration: m });
    expect(r.inline !== undefined).toBe(m === 'inline' || m === 'both');
    expect(r.strip !== undefined).toBe(m === 'strip' || m === 'both');
  });

  it('applies dim italic to captions and narration, red italic to warnings', () => {
    const c = routeMessage({ kind: 'caption', text: 'x' }, { captions: 'inline', narration: 'off' });
    expect(c.inline).toBe(`${ITALIC_DIM}x${RESET}`);
    const w = routeMessage({ kind: 'warning', text: 'bad' }, { captions: 'off', narration: 'inline' });
    expect(w.inline).toBe(`${RED_ITALIC}bad${RESET}`);
  });

  it('warnings follow --narration but are never suppressed: off goes to the strip', () => {
    const r = routeMessage({ kind: 'warning', text: 'bad' }, { captions: 'inline', narration: 'off' });
    expect(r.inline).toBeUndefined();
    expect(r.strip).toBe(`${RED_ITALIC}bad${RESET}`);
  });

  it('inline text starts on its own line and never contains a carriage return that could reach the PTY input', () => {
    const r = routeMessage({ kind: 'caption', text: 'x' }, { captions: 'inline', narration: 'off' });
    expect(r.inline).not.toMatch(/\n/);
  });
});

describe('StripBuffer', () => {
  it('keeps its last lines', () => {
    const s = new StripBuffer(3);
    for (const t of ['a', 'b', 'c', 'd']) s.push(t);
    expect(s.lines()).toEqual(['b', 'c', 'd']);
  });
  it('clears', () => {
    const s = new StripBuffer(3);
    s.push('a');
    s.clear();
    expect(s.lines()).toEqual([]);
  });
});

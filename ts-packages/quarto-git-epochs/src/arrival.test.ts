import { describe, expect, it } from 'vitest';
import { arrive, normalizeText } from './arrival.js';
import { readJson } from './fixture-io.js';
import { EVENTS } from './vocabulary.js';

const rows: any[] = readJson('decision-tables/arrival.json');

describe('arrive: shared fixture rows', () => {
  for (const r of rows) {
    it(r.id, () => {
      const { id: _id, expected, note: _note, ...input } = r;
      const out = arrive(input);
      expect(out.step).toBe(expected.step);
      expect(out.action).toBe(expected.action);
      expect(out.event).toBe(expected.event);
      expect(out.discarded ?? 0).toBe(expected.discarded ?? 0);
      if (out.event !== undefined) expect(EVENTS).toContain(out.event);
    });
  }
});

describe('arrive: edges', () => {
  const base = { peer: 'extension', kind: 'text', disk: 'S', doc: 'T', blob: 'B', attached: 'none' } as const;
  it('an attached-other doc that nobody retired runs the ordinary steps', () => {
    expect(arrive({ ...base, attached: 'other' }).step).toBe('5');
  });
  it('colocated jj does not change step 1 (there is no doc to detach from)', () => {
    expect(arrive({ ...base, doc: undefined, jj: true }).step).toBe('1');
  });
  it('a hub under ReadOnly with a clean differing doc creates instead of joining, under jj too', () => {
    const out = arrive({ ...base, peer: 'hub', policy: 'ReadOnly', disk: 'B', jj: true });
    expect(out.action).toBe('create-from-disk');
    expect(out.event).toBe('seeded');
  });
});

describe('normalizeText', () => {
  it('turns CRLF and lone CR into LF', () => {
    expect(normalizeText('a\r\nb\rc\n')).toBe('a\nb\nc\n');
  });
});

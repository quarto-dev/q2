import { describe, expect, it } from 'vitest';
import { classify } from './classify.js';
import { readJson } from './fixture-io.js';

const rows: any[] = readJson('decision-tables/classify.json');

describe('classify: shared fixture rows', () => {
  for (const r of rows) {
    it(r.id, () => {
      expect(classify({ peer: r.peer, reflog: r.reflog, changed: r.changed })).toBe(r.expected);
    });
  }
});

describe('classify: edges', () => {
  it('an empty change list is commit-type', () => {
    expect(classify({ peer: 'hub', reflog: ['checkout: moving from a to b'], changed: [] })).toBe('commit');
  });
  it('an empty reflog span with a differing disk uses the content rule', () => {
    const changed = [{ path: 'a', disk: 'x', synced: 'y', blob: 'x' }];
    expect(classify({ peer: 'hub', reflog: [], changed })).toBe('checkout');
  });
  it('a plain-commit reflog span makes the whole set commit-type; any other entry makes it checkout-type', () => {
    const changed = [
      { path: 'a', disk: 'x', synced: 'x', blob: 'x' },
      { path: 'b', disk: 'n', synced: 'o', blob: 'n' },
    ];
    expect(classify({ peer: 'extension', reflog: ['commit: m'], changed })).toBe('commit');
    expect(classify({ peer: 'extension', reflog: ['commit: m', 'reset: moving to HEAD'], changed })).toBe('checkout');
  });
  it('an initial commit subject counts as a plain commit', () => {
    const changed = [{ path: 'a', disk: 'n', synced: 'o', blob: 'n' }];
    expect(classify({ peer: 'extension', reflog: ['commit (initial): base'], changed })).toBe('commit');
  });
});

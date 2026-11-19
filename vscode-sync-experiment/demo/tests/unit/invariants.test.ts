import { describe, expect, it } from 'vitest';
import { checkInvariants, stablePointerTexts, type Snapshot } from '../../src/invariants';

const good = (): Snapshot => ({
  quiet: true,
  docs: [{ path: 'a.qmd', text: 'fine\n' }],
  hubDisk: { 'a.qmd': 'fine\n' },
  pointerDocs: { 'a.qmd': 'fine\n' },
  clones: [{ name: 'maurice', branch: 'feature', hubBranch: 'main', diskRewritten: false }],
  followers: [{ name: 'guest', shownVersion: 'v2', pointerVersion: 'v2' }],
});
const ids = (s: Snapshot) => checkInvariants(s).map((v) => v.id);

describe('invariants (DM8)', () => {
  it('a good state has no violation', () => expect(ids(good())).toEqual([]));

  it('flags a conflict marker in any doc', () => {
    const s = good();
    s.docs.push({ path: 'b.qmd', text: 'x\n<<<<<<< HEAD\ny\n=======\nz\n>>>>>>> other\n' });
    const v = checkInvariants(s);
    expect(v.map((x) => x.id)).toEqual(['conflict-marker']);
    expect(v[0]!.summary).toContain('b.qmd');
  });

  it('does not flag a line that merely starts with seven equals signs in prose', () => {
    const s = good();
    s.docs[0]!.text = 'Title\n=======\n';
    expect(ids(s)).toEqual([]);
  });

  it('flags hub disk differing from the pointed doc when quiet, not when busy', () => {
    const s = good();
    s.hubDisk['a.qmd'] = 'other\n';
    expect(ids(s)).toEqual(['disk-differs-from-pointer']);
    s.quiet = false;
    expect(ids(s)).toEqual([]);
  });

  it('flags a clone on another branch whose disk was rewritten', () => {
    const s = good();
    s.clones[0]!.diskRewritten = true;
    expect(ids(s)).toEqual(['clone-disk-rewritten']);
    s.clones[0]!.branch = 'main';
    expect(ids(s)).toEqual([]);
  });

  it('flags a follower that shows a stale version when quiet', () => {
    const s = good();
    s.followers[0]!.shownVersion = 'v1';
    expect(ids(s)).toEqual(['follower-stale']);
    s.quiet = false;
    expect(ids(s)).toEqual([]);
  });
});

describe('stablePointerTexts', () => {
  it('releases a pointed text only after it has been unchanged for the window', () => {
    const since = new Map<string, { text: string; since: number }>();
    expect(stablePointerTexts(since, { 'a.qmd': 'one' }, 1000, 3000)).toEqual({});
    expect(stablePointerTexts(since, { 'a.qmd': 'one' }, 3999, 3000)).toEqual({});
    expect(stablePointerTexts(since, { 'a.qmd': 'one' }, 4000, 3000)).toEqual({ 'a.qmd': 'one' });
  });

  it('restarts the window when the text changes, and forgets a path that left', () => {
    const since = new Map<string, { text: string; since: number }>();
    stablePointerTexts(since, { 'a.qmd': 'one', 'b.qmd': 'x' }, 0, 3000);
    expect(stablePointerTexts(since, { 'a.qmd': 'two', 'b.qmd': 'x' }, 3500, 3000)).toEqual({ 'b.qmd': 'x' });
    expect(stablePointerTexts(since, { 'a.qmd': 'two' }, 6400, 3000)).toEqual({});
    expect(since.has('b.qmd')).toBe(false);
    expect(stablePointerTexts(since, { 'a.qmd': 'two' }, 6500, 3000)).toEqual({ 'a.qmd': 'two' });
  });
});

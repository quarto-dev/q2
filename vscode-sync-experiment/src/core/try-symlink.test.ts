import { describe, expect, it } from 'vitest';
import { trySymlinkWith } from './test-support';

const refused = (code: string) => () => {
  throw Object.assign(new Error(`${code}: operation not permitted, symlink`), { code });
};

describe('trySymlinkWith (B2: a file symlink needs privilege on Windows)', () => {
  it('returns true when the link is made', () => {
    const calls: unknown[][] = [];
    expect(trySymlinkWith((...a) => void calls.push(a), 'win32', 'target', 'link', 'file')).toBe(true);
    expect(calls).toEqual([['target', 'link', 'file']]);
  });

  it('returns false when win32 refuses with EPERM', () => {
    expect(trySymlinkWith(refused('EPERM'), 'win32', 't', 'l')).toBe(false);
  });

  it('rethrows EPERM on other platforms, where it means a real problem', () => {
    expect(() => trySymlinkWith(refused('EPERM'), 'linux', 't', 'l')).toThrow(/EPERM/);
  });

  it('rethrows any other error on win32', () => {
    expect(() => trySymlinkWith(refused('EEXIST'), 'win32', 't', 'l')).toThrow(/EEXIST/);
  });
});

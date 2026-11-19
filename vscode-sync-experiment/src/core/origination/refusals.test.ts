import { mkdirSync } from 'node:fs';
import * as path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { cleanupGitTempDirs, initRepo, put, repoWith } from '../clone/git-fixtures';
import { writeProjectMeta } from '../meta';
import { tempDir } from '../test-support';
import { checkOriginate, pickFolder } from './refusals';

afterEach(() => cleanupGitTempDirs());

describe('checkOriginate', () => {
  it('accepts a git clone with a commit and a plain folder', async () => {
    expect(await checkOriginate(repoWith({ 'a.qmd': 'a' }))).toEqual({ ok: true });
    expect(await checkOriginate(await tempDir('ge12-ref-'))).toEqual({ ok: true });
  });

  it('refuses a bound folder, pointing at the share panel', async () => {
    const dir = repoWith({ 'a.qmd': 'a' });
    await writeProjectMeta(dir, { indexDocId: 'x', syncServer: 'ws://h', createdAt: 'now' });
    expect(await checkOriginate(dir)).toEqual({ ok: false, reason: 'already-shared', message: 'Already shared; showing its share information.' });
  });

  it('refuses a repo with no commits', async () => {
    const dir = initRepo();
    put(dir, 'a.qmd', 'a');
    expect(await checkOriginate(dir)).toEqual({ ok: false, reason: 'unborn', message: 'Make an initial commit first.' });
  });

  it('refuses when git is missing but the folder is inside a repo (a .git above it)', async () => {
    const dir = repoWith({ 'sub/a.qmd': 'a' });
    const r = await checkOriginate(path.join(dir, 'sub'), { gitPath: path.join(dir, 'no-such-git') });
    expect(r).toEqual({ ok: false, reason: 'git-unavailable', message: 'git is not available on the extension host.' });
  });

  it('a plain folder does not need git at all', async () => {
    const dir = await tempDir('ge12-ref-nogit-');
    mkdirSync(path.join(dir, 'x'));
    expect(await checkOriginate(dir, { gitPath: '/no/such/git' })).toEqual({ ok: true });
  });
});

describe('pickFolder', () => {
  it('returns the only folder without asking', async () => {
    expect(await pickFolder(['/a'], async () => { throw new Error('asked'); })).toBe('/a');
  });
  it('asks which one when several are open, and returns the choice (or null on cancel)', async () => {
    expect(await pickFolder(['/a', '/b'], async (c) => c[1] ?? null)).toBe('/b');
    expect(await pickFolder(['/a', '/b'], async () => null)).toBeNull();
  });
  it('is null with no folder open', async () => {
    expect(await pickFolder([], async () => '/x')).toBeNull();
  });
});

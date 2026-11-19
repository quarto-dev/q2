import { afterEach, describe, expect, it } from 'vitest';
import { cleanupGitTempDirs, git, initRepo, commitAll, put, repoWith } from '../clone/git-fixtures';
import { dirtyFiles } from './dirty';
import { selectFiles } from './select';

afterEach(() => cleanupGitTempDirs());

describe('dirtyFiles', () => {
  it('is empty for a clean checkout', async () => {
    const dir = repoWith({ 'a.qmd': 'a\n', 'b.qmd': 'b\n' });
    expect(await dirtyFiles(dir, await selectFiles(dir))).toEqual([]);
  });

  it('names a modified text file and a modified binary file, sorted', async () => {
    const dir = repoWith({ 'b.qmd': 'b\n', 'a.qmd': 'a\n', 'p.png': Buffer.from([1, 2, 3]) });
    put(dir, 'b.qmd', 'changed\n');
    put(dir, 'p.png', Buffer.from([1, 2, 4]));
    expect(await dirtyFiles(dir, await selectFiles(dir))).toEqual(['b.qmd', 'p.png']);
  });

  it('does not count a CRLF checkout of an LF blob as a change (text is normalized)', async () => {
    const dir = initRepo({ autocrlf: false });
    put(dir, 'a.qmd', 'one\ntwo\n');
    commitAll(dir);
    put(dir, 'a.qmd', 'one\r\ntwo\r\n');
    expect(git(dir, 'status', '--porcelain')).not.toBe(''); // git itself sees a change; we do not
    expect(await dirtyFiles(dir, await selectFiles(dir))).toEqual([]);
  });

  it('compares a binary by its bytes, not its text form', async () => {
    const dir = repoWith({ 'p.png': Buffer.from([0x0d, 0x0a, 0, 1]) });
    put(dir, 'p.png', Buffer.from([0x0a, 0, 1]));
    expect(await dirtyFiles(dir, await selectFiles(dir))).toEqual(['p.png']);
  });

  it('is empty for a plain folder', async () => {
    const { tempDir } = await import('../test-support');
    const dir = await tempDir('ge12-dirty-plain-');
    put(dir, 'a.qmd', 'a');
    expect(await dirtyFiles(dir, await selectFiles(dir))).toEqual([]);
  });
});

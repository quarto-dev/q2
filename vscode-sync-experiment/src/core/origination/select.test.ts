import { mkdirSync, symlinkSync, writeFileSync } from 'node:fs';
import * as path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { cleanupGitTempDirs, commitAll, git, initRepo, put, repoWith } from '../clone/git-fixtures';
import { tempDir } from '../test-support';
import { PUBLISH_CAP_BYTES, selectFiles } from './select';

afterEach(() => cleanupGitTempDirs());

const paths = (s: { files: { path: string }[] }) => s.files.map((f) => f.path).sort();

describe('selectFiles, a git clone', () => {
  it('publishes tracked files minus the ignore set, and never untracked ones', async () => {
    const dir = repoWith({
      'index.qmd': 'hi\n',
      'guides/a.qmd': 'a\n',
      '.vscode/settings.json': '{}',
      'node_modules/x/index.js': 'x',
      '_site/index.html': '<p>',
      'nested/.quarto-hub/project.json': '{}',
      'nested/inner.qmd': 'a nested bound project is not swallowed\n',
      'sub/.quarto-hub/automerge/x': 'a nested .quarto-hub at any depth',
      'sub/.git-note.txt': 'ok',
      'logo.png': Buffer.from([0x89, 0x50, 0x4e, 0x47, 0, 1, 2]),
    });
    put(dir, 'untracked.qmd', 'not tracked\n');
    const s = await selectFiles(dir);
    expect(s.kind).toBe('git');
    expect(paths(s)).toEqual(['guides/a.qmd', 'index.qmd', 'logo.png', 'sub/.git-note.txt']);
    expect(s.branch).toBe('main');
  });

  it('classifies text against binary by extension, and records the HEAD blob', async () => {
    const dir = repoWith({ 'a.qmd': 'a\n', 'p.png': Buffer.from([1, 2, 3]) });
    const s = await selectFiles(dir);
    const byPath = Object.fromEntries(s.files.map((f) => [f.path, f]));
    expect(byPath['a.qmd']!.binary).toBe(false);
    expect(byPath['p.png']!.binary).toBe(true);
    expect(byPath['a.qmd']!.blob).toBe(git(dir, 'rev-parse', 'HEAD:a.qmd'));
  });

  it('drops symlinks and submodule links', async () => {
    const dir = initRepo();
    put(dir, 'real.qmd', 'r\n');
    symlinkSync('real.qmd', path.join(dir, 'link.qmd'));
    commitAll(dir);
    git(dir, 'update-index', '--add', '--cacheinfo', '160000', git(dir, 'rev-parse', 'HEAD'), 'vendored');
    git(dir, 'commit', '-q', '-m', 'sub');
    expect(paths(await selectFiles(dir))).toEqual(['real.qmd']);
  });

  it('keeps an LFS pointer as the small text file it is', async () => {
    const pointer = 'version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 123456789\n';
    const dir = repoWith({ 'data.csv': pointer, 'a.qmd': 'a\n' });
    const s = await selectFiles(dir);
    expect(s.files.find((f) => f.path === 'data.csv')).toMatchObject({ binary: false, size: pointer.length });
  });

  it('skips a file over 10 MB and lists it, and counts what is published', async () => {
    const big = Buffer.alloc(PUBLISH_CAP_BYTES + 1, 1);
    const dir = repoWith({ 'big.bin': big, 'a.qmd': 'abc\n' });
    const s = await selectFiles(dir);
    expect(paths(s)).toEqual(['a.qmd']);
    expect(s.skipped).toEqual([{ path: 'big.bin', size: PUBLISH_CAP_BYTES + 1 }]);
    expect(s.totalBytes).toBe(4);
  });

  it('a project in a subdirectory of the repository publishes only that prefix, with project-relative paths', async () => {
    const dir = repoWith({ 'top.qmd': 't\n', 'docs/index.qmd': 'i\n', 'docs/g/b.qmd': 'b\n' });
    const s = await selectFiles(path.join(dir, 'docs'));
    expect(paths(s)).toEqual(['g/b.qmd', 'index.qmd']);
    expect(s.prefix).toBe('docs');
  });

  it('reads the on-disk size of a modified file as published (uncommitted edits go as they are)', async () => {
    const dir = repoWith({ 'a.qmd': 'a\n' });
    put(dir, 'a.qmd', 'a much longer line now\n');
    const s = await selectFiles(dir);
    expect(s.files[0]!.size).toBe('a much longer line now\n'.length);
  });
});

describe('selectFiles, a plain folder', () => {
  it('walks the folder with the extension ignore set and makes a plain project', async () => {
    const dir = await tempDir('ge12-plain-');
    for (const [p, c] of Object.entries({ 'index.qmd': 'i', 'a/b.qmd': 'b', '.git-ish': 'x', 'node_modules/m.js': 'm', '.quarto-hub/project.json': '{}', 'nested/.quarto-hub/project.json': '{}', 'nested/c.qmd': 'c' })) {
      mkdirSync(path.dirname(path.join(dir, p)), { recursive: true });
      writeFileSync(path.join(dir, p), c);
    }
    const s = await selectFiles(dir);
    expect(s.kind).toBe('plain');
    expect(s.branch).toBeNull();
    expect(paths(s)).toEqual(['.git-ish', 'a/b.qmd', 'index.qmd']);
  });
});

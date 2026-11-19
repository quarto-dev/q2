/** GE-10 specs 1 to 4: attach preconditions, identity check, prefix mapping, the exclude file. */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { excludeMetaDir } from '../git-exclude';
import { META_DIR } from '../ignore';
import { META_FILE } from '../meta';
import { checkAttach } from './attach';
import { cleanupGitTempDirs, cloneRepo, commitAll, git, gitTempDir, put, repoWith, rootCommits } from './git-fixtures';

afterEach(() => cleanupGitTempDirs());

const FILES = { 'index.qmd': 'hi\n', 'a.qmd': 'a\n', 'b.qmd': 'b\n', 'c.qmd': 'c\n', 'd.qmd': 'd\n' };

function shareOf(repo: string, prefix = '') {
  return { repo: { roots: rootCommits(repo), prefix } };
}

describe('attach preconditions (spec 1)', () => {
  it('refuses when the share has no repo entry', async () => {
    const dir = repoWith(FILES);
    const r = await checkAttach({ dir, share: {} });
    expect(r).toMatchObject({ ok: false, reason: 'no-repo-entry', offerMirror: true });
    expect(!r.ok && r.message).toMatch(/git/i);
  });

  it('refuses when the folder is not in a git repository', async () => {
    const dir = gitTempDir();
    const r = await checkAttach({ dir, share: { repo: { roots: ['abc'], prefix: '' } } });
    expect(r).toMatchObject({ ok: false, reason: 'not-a-repo', offerMirror: true });
  });

  it('refuses when the folder already has .quarto-hub/project.json', async () => {
    const dir = repoWith(FILES);
    await fs.mkdir(path.join(dir, META_DIR));
    await fs.writeFile(path.join(dir, META_DIR, META_FILE), '{}');
    const r = await checkAttach({ dir, share: shareOf(dir) });
    expect(r).toMatchObject({ ok: false, reason: 'already-bound', offerMirror: false });
  });
});

describe('identity check (spec 2)', () => {
  it('passes for a clone of the project repository', async () => {
    const origin = repoWith(FILES);
    const clone = cloneRepo(origin);
    const r = await checkAttach({ dir: clone, share: shareOf(origin) });
    expect(r).toMatchObject({ ok: true, projectRoot: clone });
  });

  it('fails for an unrelated repository that also has an index.qmd, and offers a mirror', async () => {
    const origin = repoWith(FILES);
    const other = repoWith(FILES, 'unrelated history');
    const r = await checkAttach({ dir: other, share: shareOf(origin) });
    expect(r).toMatchObject({ ok: false, reason: 'identity-mismatch', offerMirror: true });
  });

  it('passes on any intersection of root commits', async () => {
    const a = repoWith(FILES);
    const b = repoWith({ 'other.qmd': 'x\n' });
    const share = { repo: { roots: [...rootCommits(a), ...rootCommits(b)], prefix: '' } };
    expect(await checkAttach({ dir: b, share })).toMatchObject({ ok: true });
  });

  it('a shallow clone fails the root check and passes the blob overlap fallback at half the files', async () => {
    const origin = repoWith(FILES);
    put(origin, 'later.qmd', 'later\n');
    commitAll(origin, 'second');
    const shallow = cloneRepo(origin, ['--depth', '1']);
    const tracked = git(origin, 'ls-tree', '-r', 'HEAD').split('\n').map((l) => ({ blob: l.split(/\s+/)[2]!, path: l.split('\t')[1]! }));
    // The shallow clone's one commit is its own root, so the root commits differ from the origin's.
    const share = { repo: { roots: rootCommits(origin), prefix: '' }, registry: tracked };
    expect(await checkAttach({ dir: shallow, share })).toMatchObject({ ok: true });
    // Below half: most registry entries name blobs the clone does not have.
    const stale = tracked.map((e, i) => (i < 4 ? { ...e, blob: 'f'.repeat(40) } : e));
    const bad = await checkAttach({ dir: shallow, share: { ...share, registry: stale } });
    expect(bad).toMatchObject({ ok: false, reason: 'identity-mismatch', offerMirror: true });
  });
});

describe('prefix mapping (spec 3)', () => {
  it('the project in a subdirectory gives the same project root whether opened at the repo root or the subdirectory', async () => {
    const repo = repoWith({ 'site/index.qmd': 'hi\n', 'other/x.qmd': 'x\n' });
    const share = shareOf(repo, 'site');
    const fromRoot = await checkAttach({ dir: repo, share });
    const fromSub = await checkAttach({ dir: path.join(repo, 'site'), share });
    expect(fromRoot).toMatchObject({ ok: true, projectRoot: path.join(repo, 'site') });
    expect(fromSub).toMatchObject({ ok: true, projectRoot: path.join(repo, 'site') });
  });

  it('a different subdirectory is refused with a message', async () => {
    const repo = repoWith({ 'site/index.qmd': 'hi\n', 'other/x.qmd': 'x\n' });
    const r = await checkAttach({ dir: path.join(repo, 'other'), share: shareOf(repo, 'site') });
    expect(r).toMatchObject({ ok: false, reason: 'wrong-subdirectory' });
    expect(!r.ok && r.message).toContain('site');
  });
});

describe('exclude file (spec 4)', () => {
  it('is written once and idempotently, also in a linked worktree', async () => {
    const repo = repoWith(FILES);
    const wt = gitTempDir('ge10-wt-');
    await fs.rm(wt, { recursive: true });
    git(repo, 'worktree', 'add', '-q', '-b', 'side', wt);
    expect(await excludeMetaDir(wt)).toBe(true);
    expect(await excludeMetaDir(wt)).toBe(false);
    await fs.mkdir(path.join(wt, META_DIR));
    await fs.writeFile(path.join(wt, META_DIR, 'x'), '1');
    expect(git(wt, 'status', '--porcelain')).not.toContain(META_DIR);
  });
});

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { GitUnavailableError } from './probe-types.js';
import { contentKey, createGitProbe } from './probe.js';
import { cleanupTempDirs, commitAll, git, hasCommand, initRepo, put, tempDir } from './repo-helper.js';

afterAll(cleanupTempDirs);

describe('probe: HEAD and identity', () => {
  it('reports null for an unborn HEAD and the commit id after the first commit', async () => {
    const dir = initRepo();
    const p = createGitProbe({ cwd: dir });
    expect(await p.headSha()).toBeNull();
    put(dir, 'a.qmd', 'one\n');
    const sha = commitAll(dir, 'base');
    expect(await p.headSha()).toBe(sha);
  });

  it('showPrefix is empty at the root and the sub path (no trailing slash) in a subdirectory', async () => {
    const dir = initRepo();
    put(dir, 'site/a.qmd', 'x\n');
    commitAll(dir, 'base');
    expect(await createGitProbe({ cwd: dir }).showPrefix()).toBe('');
    expect(await createGitProbe({ cwd: join(dir, 'site') }).showPrefix()).toBe('site');
  });

  it('rootCommits finds two roots', async () => {
    const dir = initRepo();
    put(dir, 'a.qmd', '1\n');
    const r1 = commitAll(dir, 'one');
    git(dir, 'switch', '-q', '--orphan', 'other');
    put(dir, 'b.qmd', '2\n');
    const r2 = commitAll(dir, 'two');
    git(dir, 'merge', '-q', '--allow-unrelated-histories', '-m', 'merge', 'main');
    const roots = await createGitProbe({ cwd: dir }).rootCommits();
    expect(roots.sort()).toEqual([r1, r2].sort());
  });

  it('isShallow is false for a repo and true for a --depth 1 clone through a file:// URL', async () => {
    const src = initRepo();
    put(src, 'a.qmd', '1\n');
    commitAll(src, 'one');
    put(src, 'a.qmd', '2\n');
    commitAll(src, 'two');
    const dest = join(tempDir(), 'clone');
    execFileSync('git', ['clone', '-q', '--depth', '1', `file://${src.startsWith('/') ? '' : '/'}${src.replace(/\\/g, '/')}`, dest]);
    expect(await createGitProbe({ cwd: src }).isShallow()).toBe(false);
    expect(await createGitProbe({ cwd: dest }).isShallow()).toBe(true);
  });
});

describe('probe: ls-tree and cat-file', () => {
  it('lists regular files under the prefix only, with modes 100644 and 100755, filtering symlinks and submodules', async () => {
    const dir = initRepo();
    put(dir, 'site/a.qmd', 'a\n');
    put(dir, 'site/deep/b.qmd', 'b\n');
    put(dir, 'site/run.sh', '#!/bin/sh\n');
    put(dir, 'other/x.qmd', 'x\n');
    git(dir, 'add', '-A');
    git(dir, 'update-index', '--chmod=+x', 'site/run.sh');
    const blob = git(dir, 'rev-parse', ':site/a.qmd');
    git(dir, 'update-index', '--add', '--cacheinfo', `120000,${blob},site/link`); // a symlink entry, on any platform
    git(dir, 'update-index', '--add', '--cacheinfo', `160000,${'1'.repeat(40)},site/sub`); // a gitlink (submodule)
    git(dir, 'commit', '-q', '-m', 'base');
    const files = await createGitProbe({ cwd: join(dir, 'site') }).lsTree();
    expect(files.map((f) => f.path).sort()).toEqual(['a.qmd', 'deep/b.qmd', 'run.sh']);
    expect(files.find((f) => f.path === 'run.sh')!.mode).toBe('100755');
    expect(files.find((f) => f.path === 'a.qmd')!.blob).toBe(blob);
  });

  it('lists nothing for an unborn repository', async () => {
    expect(await createGitProbe({ cwd: initRepo() }).lsTree()).toEqual([]);
  });

  it('cat-file --batch returns blob bytes; a missing id is absent; with core.autocrlf the stored blob is LF', async () => {
    const dir = initRepo({ autocrlf: true });
    put(dir, 'a.qmd', 'one\r\ntwo\r\n');
    put(dir, 'bin.dat', Buffer.from([0, 255, 1, 2, 10, 13]));
    commitAll(dir, 'base');
    const p = createGitProbe({ cwd: dir });
    const files = await p.lsTree();
    const a = files.find((f) => f.path === 'a.qmd')!;
    const bin = files.find((f) => f.path === 'bin.dat')!;
    const got = await p.catBlobs([a.blob, bin.blob, '0'.repeat(40)]);
    expect(Buffer.from(got.get(a.blob)!).toString('utf8')).toBe('one\ntwo\n');
    expect(Array.from(got.get(bin.blob)!)).toEqual([0, 255, 1, 2, 10, 13]);
    expect(got.has('0'.repeat(40))).toBe(false);
  });

  it('without core.autocrlf the CRLF bytes are stored as written', async () => {
    const dir = initRepo({ autocrlf: false });
    put(dir, 'a.qmd', 'one\r\n');
    commitAll(dir, 'base');
    const p = createGitProbe({ cwd: dir });
    const a = (await p.lsTree())[0];
    expect(Buffer.from((await p.catBlobs([a.blob])).get(a.blob)!).toString('utf8')).toBe('one\r\n');
  });
});

describe('probe: reflog', () => {
  function repoWithHistory() {
    const dir = initRepo();
    put(dir, 'a.qmd', '1\n');
    const c1 = commitAll(dir, 'base');
    git(dir, 'switch', '-q', '-c', 'side');
    put(dir, 'a.qmd', '2\n');
    const c2 = commitAll(dir, 'side');
    git(dir, 'switch', '-q', 'main');
    return { dir, c1, c2 };
  }

  it('returns every subject for a null mark and the tail mark of the newest entry', async () => {
    const { dir, c1 } = repoWithHistory();
    const s = (await createGitProbe({ cwd: dir }).reflogSince(null))!;
    expect(s.subjects).toHaveLength(4);
    expect(s.subjects[0]).toMatch(/^commit \(initial\): base/);
    expect(s.subjects[3]).toMatch(/^checkout: moving from side to main/);
    expect(s.tail).toEqual({ sha: c1, lines: 4 });
    expect(s.markFound).toBe(true);
  });

  it('returns the entries after a mark that appears twice: the last occurrence without a line count, the exact one with it', async () => {
    const { dir, c1 } = repoWithHistory();
    const p = createGitProbe({ cwd: dir });
    expect((await p.reflogSince({ sha: c1 }))!.subjects).toEqual([]); // last occurrence is the newest entry
    const exact = (await p.reflogSince({ sha: c1, lines: 1 }))!;
    expect(exact.subjects).toHaveLength(3);
  });

  it('returns every entry with markFound false when the sha is absent', async () => {
    const { dir } = repoWithHistory();
    const s = (await createGitProbe({ cwd: dir }).reflogSince({ sha: 'f'.repeat(40) }))!;
    expect(s.markFound).toBe(false);
    expect(s.subjects).toHaveLength(4);
  });

  it('picks up entries appended after a recorded tail', async () => {
    const { dir } = repoWithHistory();
    const p = createGitProbe({ cwd: dir });
    const tail = (await p.reflogSince(null))!.tail;
    git(dir, 'switch', '-q', 'side');
    const s = (await p.reflogSince(tail))!;
    expect(s.subjects).toHaveLength(1);
    expect(s.subjects[0]).toMatch(/^checkout: moving from main to side/);
  });

  it('returns null when there is no reflog, and cheapState reports no reflog without error', async () => {
    const dir = initRepo();
    git(dir, 'config', 'core.logAllRefUpdates', 'false');
    put(dir, 'a.qmd', '1\n');
    commitAll(dir, 'base');
    rmSync(join(dir, '.git', 'logs'), { recursive: true, force: true });
    const p = createGitProbe({ cwd: dir });
    await p.ready();
    expect(await p.reflogSince(null)).toBeNull();
    expect(p.cheapState().reflog).toBeNull();
  });
});

describe('probe: markers and cheapState', () => {
  it('sees index.lock in the git dir, and resolves it through --git-path for a linked worktree', async () => {
    const dir = initRepo();
    put(dir, 'a.qmd', '1\n');
    commitAll(dir, 'base');
    const wt = join(tempDir(), 'wt');
    git(dir, 'worktree', 'add', '-q', wt, '-b', 'wt-branch');
    const p = createGitProbe({ cwd: wt });
    await p.ready();
    expect(p.cheapState().markers).toEqual([]);
    const lockPath = git(wt, 'rev-parse', '--path-format=absolute', '--git-path', 'index.lock');
    expect(lockPath).toContain('worktrees');
    writeFileSync(lockPath, '');
    const m = p.cheapState().markers;
    expect(m.map((x) => [x.name, x.kind])).toEqual([['index.lock', 'lock']]);
    expect(m[0].mtimeMs).toBeGreaterThan(0);
  });

  it('sees the rebase markers of a stopped rebase as state markers', async () => {
    const dir = initRepo();
    put(dir, 'a.qmd', 'one\n');
    commitAll(dir, 'base');
    git(dir, 'switch', '-q', '-c', 'side');
    put(dir, 'a.qmd', 'side\n');
    commitAll(dir, 'side');
    git(dir, 'switch', '-q', 'main');
    put(dir, 'a.qmd', 'main2\n');
    commitAll(dir, 'main2');
    git(dir, 'switch', '-q', 'side');
    expect(() => git(dir, 'rebase', 'main')).toThrow();
    const p = createGitProbe({ cwd: dir });
    await p.ready();
    const names = p.cheapState().markers.map((x) => [x.name, x.kind]);
    expect(names).toContainEqual(['rebase-merge', 'state']);
    git(dir, 'rebase', '--abort');
    expect(p.cheapState().markers).toEqual([]);
  });

  it('sees a merge in progress (MERGE_HEAD)', async () => {
    const dir = initRepo();
    put(dir, 'a.qmd', 'one\n');
    commitAll(dir, 'base');
    git(dir, 'switch', '-q', '-c', 'side');
    put(dir, 'a.qmd', 'side\n');
    commitAll(dir, 'side');
    git(dir, 'switch', '-q', 'main');
    put(dir, 'a.qmd', 'main2\n');
    commitAll(dir, 'main2');
    expect(() => git(dir, 'merge', 'side')).toThrow();
    const p = createGitProbe({ cwd: dir });
    await p.ready();
    expect(p.cheapState().markers.map((x) => x.name)).toContain('MERGE_HEAD');
  });

  it('treats the colocated jj lock as a lock marker', async () => {
    const dir = initRepo();
    put(dir, 'a.qmd', '1\n');
    commitAll(dir, 'base');
    mkdirSync(join(dir, '.jj', 'working_copy'), { recursive: true });
    const p = createGitProbe({ cwd: dir });
    await p.ready();
    expect(p.isColocatedJj()).toBe(true);
    expect(p.cheapState().markers).toEqual([]);
    writeFileSync(join(dir, '.jj', 'working_copy', 'working_copy.lock'), '');
    expect(p.cheapState().markers.map((x) => [x.name, x.kind])).toEqual([['.jj/working_copy/working_copy.lock', 'lock']]);
  });

  it.skipIf(!hasCommand('jj'))('detects a real colocated jj repository', async () => {
    const dir = initRepo();
    put(dir, 'a.qmd', '1\n');
    commitAll(dir, 'base');
    execFileSync('jj', ['git', 'init', '--colocate'], { cwd: dir, stdio: 'ignore' });
    const p = createGitProbe({ cwd: dir });
    await p.ready();
    expect(p.isColocatedJj()).toBe(true);
    expect(existsSync(join(dir, '.jj'))).toBe(true);
  });
});

describe('probe: failure modes', () => {
  it('a missing git yields GitUnavailableError', async () => {
    const p = createGitProbe({ cwd: tempDir(), gitPath: join(tempDir(), 'no-such-git') });
    await expect(p.headSha()).rejects.toBeInstanceOf(GitUnavailableError);
  });

  it('a directory that is not a repository yields GitUnavailableError', async () => {
    await expect(createGitProbe({ cwd: tempDir() }).headSha()).rejects.toBeInstanceOf(GitUnavailableError);
  });

  it('a call that exceeds its timeout rejects', async () => {
    const dir = initRepo();
    const p = createGitProbe({ cwd: dir, timeoutMs: 1, gitPath: process.execPath });
    await expect(p.rootCommits()).rejects.toThrow();
  });
});

describe('contentKey', () => {
  it('normalizes text to LF and hashes binary bytes', () => {
    expect(contentKey(Buffer.from('a\r\nb\r\n'))).toBe('a\nb\n');
    const k = contentKey(Buffer.from([0, 1, 2]));
    expect(k).toMatch(/^sha256:[0-9a-f]{64}$/);
    expect(contentKey(Buffer.from([0, 1, 2]))).toBe(k);
    expect(contentKey(Buffer.from([0, 1, 3]))).not.toBe(k);
  });
});

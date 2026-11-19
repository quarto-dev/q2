import { realpathSync } from 'node:fs';
import * as fs from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { UnsafePathError, isReservedShortAlias, safeJoin } from './safe-path';

// A passthrough `realpath` that a test can make fail with a chosen error (the Windows run saw a transient failure under load).
const injected = vi.hoisted(() => ({ err: undefined as (Error & { code?: string }) | undefined }));
vi.mock('node:fs/promises', async (orig) => {
  const actual = await orig<typeof import('node:fs/promises')>();
  return { ...actual, realpath: (...a: Parameters<typeof actual.realpath>) => (injected.err ? Promise.reject(injected.err) : actual.realpath(...a)) };
});

let root: string;
let outside: string;

beforeEach(async () => {
  const base = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'ge-safe-')));
  root = path.join(base, 'root');
  outside = path.join(base, 'outside');
  await fs.mkdir(root);
  await fs.mkdir(outside);
});

afterEach(async () => {
  injected.err = undefined;
  await fs.rm(path.dirname(root), { recursive: true, force: true });
});

describe('safeJoin accepts (G36)', () => {
  it.each([
    ['a.qmd', 'a.qmd'],
    ['dir/a.qmd', path.join('dir', 'a.qmd')],
    ['dir/../a.qmd', 'a.qmd'],
    ['a/b/c/d.png', path.join('a', 'b', 'c', 'd.png')],
    ['.gitignore', '.gitignore'],
    ['x.git/y', path.join('x.git', 'y')],
  ])('%s', async (rel, expected) => {
    expect(await safeJoin(root, rel)).toBe(path.join(root, expected));
  });
});

describe('safeJoin rejects (G36)', () => {
  const hostile: [string, string][] = [
    ['parent traversal', '../x'],
    ['deep traversal', 'a/../../x'],
    ['an absolute path', '/abs'],
    ['a drive path', 'C:\\x'],
    ['a UNC path', '\\\\server\\share\\x'],
    ['a backslash in a key', 'a\\b'],
    ['a NUL byte', 'a\u0000b.qmd'],
    ['a .git hook', '.git/hooks/h'],
    ['.git in another case', '.GIT/x'],
    ['a nested .quarto-hub', 'sub/.quarto-hub/x'],
    ['.vscode tasks', '.vscode/tasks.json'],
    ['.git with a trailing dot', '.git./x'],
    ['.git with a trailing space', '.git /x'],
    ['.git with an alternate data stream', '.git::$INDEX_ALLOCATION/x'],
    ['a colon in a segment', 'a/b:c.qmd'],
    ['a segment ending in a dot', 'a/b./c'],
    ['a segment ending in a space', 'a/b /c'],
    ['an empty key', ''],
    ['the root itself', '.'],
    ['a traversal that lands on .git', 'dir/../.git/config'],
  ];
  it.each(hostile)('%s', async (_name, rel) => {
    await expect(safeJoin(root, rel)).rejects.toBeInstanceOf(UnsafePathError);
  });

  it('a symlinked parent directory that points outside the root', async (ctx) => {
    try {
      // 'junction' needs no privilege on Windows and is ignored elsewhere.
      await fs.symlink(outside, path.join(root, 'link'), 'junction');
    } catch {
      ctx.skip();
    }
    await expect(safeJoin(root, 'link/x.qmd')).rejects.toBeInstanceOf(UnsafePathError);
    // A path below a not-yet-existing child of the link is rejected too.
    await expect(safeJoin(root, 'link/new/dir/x.qmd')).rejects.toBeInstanceOf(UnsafePathError);
  });

  it('allows a symlinked directory that stays inside the root', async (ctx) => {
    await fs.mkdir(path.join(root, 'real'));
    try {
      await fs.symlink(path.join(root, 'real'), path.join(root, 'alias'), 'junction');
    } catch {
      ctx.skip();
    }
    expect(await safeJoin(root, 'alias/x.qmd')).toBe(path.join(root, 'alias', 'x.qmd'));
  });

  it('a path over 260 characters is accepted and can be written (long paths)', async () => {
    // Over MAX_PATH on Windows. Node uses the extended-length form itself, so
    // this works without core.longpaths; the test pins that safeJoin and a
    // write through its result keep working for deep keys.
    const rel = `${'d'.repeat(60)}/${'e'.repeat(60)}/${'f'.repeat(60)}/${'g'.repeat(60)}/${'h'.repeat(60)}/x.qmd`;
    expect(rel.length).toBeGreaterThan(260);
    const target = await safeJoin(root, rel);
    expect(target).toBe(path.join(root, ...rel.split('/')));
    await fs.mkdir(path.dirname(target), { recursive: true });
    await fs.writeFile(target, 'x');
    expect(await fs.readFile(target, 'utf8')).toBe('x');
  });

  it('an 8.3 short name of .git is rejected where the volume has short names (windows only)', async (ctx) => {
    if (process.platform !== 'win32') ctx.skip();
    await fs.mkdir(path.join(root, '.git'));
    let resolved = '';
    try {
      resolved = realpathSync.native(path.join(root, 'GIT~1'));
    } catch {
      ctx.skip(); // the volume has no 8.3 names
    }
    if (path.basename(resolved) !== '.git') ctx.skip();
    await expect(safeJoin(root, 'GIT~1/hooks/pre-commit')).rejects.toBeInstanceOf(UnsafePathError);
  });
});

describe('isReservedShortAlias (B1: NTFS 8.3 aliases of never-synced names)', () => {
  it.each(['GIT~1', 'git~2', 'Git~10', 'GIT~1.bak', 'QUARTO~1', 'quarto~3', 'QUARTO~1.x'])('flags %s', (seg) => {
    expect(isReservedShortAlias(seg)).toBe(true);
  });

  it.each(['git', '.git', 'gitx~1', 'notes~1.qmd', 'a~b', 'GITHUB~1', 'ab~1', '~1', 'quarto', 'x.git'])('leaves %s alone', (seg) => {
    expect(isReservedShortAlias(seg)).toBe(false);
  });

  it.each(['GIT~1/hooks/pre-commit', 'sub/git~2/config', 'QUARTO~1/project.json'])('safeJoin rejects %s on every platform', async (rel) => {
    await expect(safeJoin(root, rel)).rejects.toBeInstanceOf(UnsafePathError);
  });
});

describe('safeJoin does not read a failing realpath as "the path is missing"', () => {
  it('rethrows EMFILE instead of walking up to the drive root and calling the path unsafe', async () => {
    injected.err = Object.assign(new Error('EMFILE: too many open files'), { code: 'EMFILE' });
    const e = await safeJoin(root, 'd15/f1335.qmd').catch((x: unknown) => x);
    expect(e).not.toBeInstanceOf(UnsafePathError);
    expect(e).toMatchObject({ code: 'EMFILE' });
  });

  it('still joins a path whose ancestors do not exist yet (ENOENT is "missing")', async () => {
    expect(await safeJoin(root, 'new/dir/a.qmd')).toBe(path.join(root, 'new', 'dir', 'a.qmd'));
  });
});

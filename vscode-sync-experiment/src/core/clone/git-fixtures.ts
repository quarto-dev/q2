/**
 * Test-only: real temporary git repositories for the clone-mode tests (GE-10). The identity and `core.autocrlf` are
 * set per repository; no test depends on a global git configuration. Not part of the extension bundle.
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';

const made: string[] = [];

export function gitTempDir(prefix = 'ge10-git-'): string {
  const d = realpathSync(mkdtempSync(join(tmpdir(), prefix)));
  made.push(d);
  return d;
}

export function cleanupGitTempDirs(): void {
  for (const d of made.splice(0)) rmSync(d, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 });
}

export function git(cwd: string, ...args: string[]): string {
  return execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
}

export function put(dir: string, rel: string, content: string | Buffer): void {
  const p = join(dir, rel);
  mkdirSync(dirname(p), { recursive: true });
  writeFileSync(p, content);
}

export function initRepo(opts: { autocrlf?: boolean; dir?: string } = {}): string {
  const dir = opts.dir ?? gitTempDir();
  git(dir, 'init', '-q', '-b', 'main');
  git(dir, 'config', 'user.email', 't@example.invalid');
  git(dir, 'config', 'user.name', 'T');
  git(dir, 'config', 'commit.gpgsign', 'false');
  git(dir, 'config', 'core.autocrlf', String(opts.autocrlf ?? false));
  return dir;
}

export function commitAll(dir: string, msg = 'c'): string {
  git(dir, 'add', '-A');
  git(dir, 'commit', '-q', '--allow-empty', '-m', msg);
  return git(dir, 'rev-parse', 'HEAD');
}

/** A repository on `main` with `files` committed as one commit. */
export function repoWith(files: Record<string, string | Buffer>, message = 'initial'): string {
  const dir = initRepo();
  for (const [p, c] of Object.entries(files)) put(dir, p, c);
  commitAll(dir, message);
  return dir;
}

export function rootCommits(dir: string): string[] {
  return git(dir, 'rev-list', '--max-parents=0', 'HEAD').split('\n').filter(Boolean);
}

/**
 * `git clone` of `src` into a fresh temp directory, with a per-repo identity. `core.autocrlf` is passed to `clone`
 * itself (and kept in the clone's config): the files are written during the clone, so a global `core.autocrlf=true`
 * (the Windows CI runner's default) would otherwise give the clone CRLF files before any per-repo config exists.
 */
export function cloneRepo(src: string, args: string[] = []): string {
  const dest = gitTempDir('ge10-clone-');
  git(dest, 'clone', '-q', '-c', 'core.autocrlf=false', ...args, `file://${src}`, '.');
  git(dest, 'config', 'user.email', 't@example.invalid');
  git(dest, 'config', 'user.name', 'T');
  git(dest, 'config', 'commit.gpgsign', 'false');
  return dest;
}

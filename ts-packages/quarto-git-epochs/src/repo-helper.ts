/** Test helper: temp repos under the OS temp directory, removed in teardown. */
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';

const made: string[] = [];

export function tempDir(prefix = 'ge-probe-'): string {
  const d = realpathSync(mkdtempSync(join(tmpdir(), prefix)));
  made.push(d);
  return d;
}

export function cleanupTempDirs(): void {
  for (const d of made.splice(0)) rmSync(d, { recursive: true, force: true });
}

export function git(cwd: string, ...args: string[]): string {
  return execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
}

export function initRepo(opts: { autocrlf?: boolean } = {}): string {
  const dir = tempDir();
  git(dir, 'init', '-q', '-b', 'main');
  git(dir, 'config', 'user.email', 't@example.com');
  git(dir, 'config', 'user.name', 'T');
  git(dir, 'config', 'core.autocrlf', String(opts.autocrlf ?? false));
  return dir;
}

export function put(dir: string, path: string, content: string | Buffer): void {
  const p = join(dir, path);
  mkdirSync(dirname(p), { recursive: true });
  writeFileSync(p, content);
}

export function commitAll(dir: string, msg: string): string {
  git(dir, 'add', '-A');
  git(dir, 'commit', '-q', '-m', msg);
  return git(dir, 'rev-parse', 'HEAD');
}

export function hasCommand(cmd: string, arg = '--version'): boolean {
  try {
    execFileSync(cmd, [arg], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

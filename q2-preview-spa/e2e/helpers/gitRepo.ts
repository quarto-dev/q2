/**
 * A real git repository for the git-epochs e2e specs (GE-13).
 *
 * Every call is `execFileSync` with an argument array, never a shell. The repository has a fixed author, no autocrlf,
 * and the global and system git config pointed at an empty file, so the developer's own config (signing, hooks,
 * templates, `init.defaultBranch`) cannot change a result.
 */

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { crc32, deflateSync } from 'node:zlib';
import { tmpdir } from 'node:os';
import path from 'node:path';

/** An empty config file shared by every repo: git reads it as the (empty) global and system config. */
function emptyConfig(): string {
  const dir = mkdtempSync(path.join(tmpdir(), 'q2-gitcfg-'));
  const file = path.join(dir, 'config');
  writeFileSync(file, '');
  return file;
}

const EMPTY_CONFIG = emptyConfig();

/** Built per call, so a changed `PATH` is seen. */
const gitEnv = (): NodeJS.ProcessEnv => ({
  ...process.env,
  GIT_CONFIG_GLOBAL: EMPTY_CONFIG,
  GIT_CONFIG_SYSTEM: EMPTY_CONFIG,
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_TERMINAL_PROMPT: '0',
  GIT_EDITOR: 'true',
});

/** Thrown with git's stderr when a command fails. */
export class GitError extends Error {
  constructor(
    readonly args: string[],
    readonly status: number | null,
    readonly stderr: string,
  ) {
    super(`git ${args.join(' ')} failed (${status}): ${stderr}`);
  }
}

function run(cmd: string, args: string[], cwd: string, allowFailure = false): { status: number; stdout: string; stderr: string } {
  try {
    const stdout = execFileSync(cmd, args, { cwd, env: gitEnv(), encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
    return { status: 0, stdout, stderr: '' };
  } catch (e) {
    const err = e as { status?: number | null; stdout?: string; stderr?: string };
    if (allowFailure) return { status: err.status ?? 1, stdout: err.stdout ?? '', stderr: err.stderr ?? '' };
    throw new GitError(args, err.status ?? null, err.stderr ?? String(e));
  }
}

/** True when `jj --version` runs. The jj scenarios skip when it does not. */
export function jjAvailable(): boolean {
  try {
    execFileSync('jj', ['--version'], { stdio: 'ignore', env: gitEnv() });
    return true;
  } catch {
    return false;
  }
}

export class GitRepo {
  constructor(readonly dir: string) {}

  /** Create the repository (branch `main`) in `dir`, which must exist. */
  static init(dir: string): GitRepo {
    run('git', ['init', '--quiet', '--initial-branch=main'], dir);
    const repo = new GitRepo(dir);
    repo.git('config', 'user.name', 'GE Test');
    repo.git('config', 'user.email', 'ge-test@example.invalid');
    repo.git('config', 'core.autocrlf', 'false');
    repo.git('config', 'commit.gpgsign', 'false');
    return repo;
  }

  git(...args: string[]): string {
    return run('git', args, this.dir).stdout;
  }

  /** Write `content` to `rel` (creating folders). A string is UTF-8; a Buffer is written as is. */
  writeFile(rel: string, content: string | Buffer): void {
    const abs = path.join(this.dir, rel);
    mkdirSync(path.dirname(abs), { recursive: true });
    writeFileSync(abs, content);
  }

  /** Stage everything (including removals) and commit; returns the new commit's hash. */
  commitAll(message: string): string {
    this.git('add', '-A');
    this.git('commit', '--quiet', '--allow-empty', '-m', message);
    return this.head();
  }

  head(): string {
    return this.git('rev-parse', 'HEAD').trim();
  }

  switch(branch: string): void {
    this.git('switch', '--quiet', branch);
  }

  switchCreate(branch: string): void {
    this.git('switch', '--quiet', '-c', branch);
  }

  resetHard(rev = 'HEAD'): void {
    this.git('reset', '--hard', '--quiet', rev);
  }

  /**
   * Start `git rebase <onto>`. Returns `{ stopped: true }` (and does not throw) when git stops for conflicts, so a
   * spec can observe the paused state; any other failure throws.
   */
  rebase(onto: string): { stopped: boolean } {
    const r = run('git', ['rebase', onto], this.dir, true);
    if (r.status === 0) return { stopped: false };
    if (this.rebaseInProgress()) return { stopped: true };
    throw new GitError(['rebase', onto], r.status, r.stderr);
  }

  rebaseInProgress(): boolean {
    return existsSync(this.markerPath('rebase-merge')) || existsSync(this.markerPath('rebase-apply'));
  }

  /** `git rebase --continue` after the conflict is resolved and staged. Returns whether another stop followed. */
  rebaseContinue(): { stopped: boolean } {
    this.git('add', '-A');
    const r = run('git', ['rebase', '--continue'], this.dir, true);
    if (r.status === 0) return { stopped: false };
    if (this.rebaseInProgress()) return { stopped: true };
    throw new GitError(['rebase', '--continue'], r.status, r.stderr);
  }

  rebaseAbort(): void {
    this.git('rebase', '--abort');
  }

  /** Absolute path of a file or folder inside the git dir (`git rev-parse --git-path`), e.g. `index.lock`. */
  markerPath(name: string): string {
    const p = this.git('rev-parse', '--git-path', name).trim();
    return path.isAbsolute(p) ? p : path.join(this.dir, p);
  }

  /** `jj git init --colocate` over the existing repository. */
  jjColocate(): void {
    run('jj', ['git', 'init', '--colocate'], this.dir);
  }

  /** `jj new <rev>`: a new working-copy commit on top of `rev`. Writes the git reflog (the trigger fires). */
  jjNew(rev: string): void {
    run('jj', ['new', rev], this.dir);
  }

  /** `jj edit <rev>`. Changes the tree with no reflog change (a stated gap, G35). */
  jjEdit(rev: string): void {
    run('jj', ['edit', rev], this.dir);
  }
}

/** The files of the shared fixture: `main` and a `feature` branch that changes, removes, and adds files. */
export interface FixtureBranches {
  /** Bytes of `img/logo.png` on `main` and on `feature`. */
  logoMain: Buffer;
  logoFeature: Buffer;
}

/** A tiny but valid, distinct PNG per `tag` (the bytes differ, which is all the specs need). */
export function pngBytes(tag: number): Buffer {
  // 1x1 RGB PNG with the pixel's red channel set to `tag`; CRCs are precomputed per tag so the file stays valid.
  const chunk = (type: string, data: Buffer): Buffer => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([len, body, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(1, 0);
  ihdr.writeUInt32BE(1, 4);
  ihdr.set([8, 2, 0, 0, 0], 8);
  const raw = Buffer.from([0, tag & 0xff, 0, 0]);
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

export const MAIN_FILES: Record<string, string> = {
  '_quarto.yml': 'project:\n  type: default\n',
  'index.qmd': '---\ntitle: Home\n---\n\nHome page.\n',
  'a.qmd': '---\ntitle: A\n---\n\nVersion MAIN of a.\n',
  'c.qmd': '---\ntitle: C\n---\n\nThe file c exists on main.\n',
};

export const FEATURE_A = '---\ntitle: A\n---\n\nVersion FEATURE of a.\n';
export const FEATURE_B = '---\ntitle: B\n---\n\nThe file b exists on feature.\n';

/**
 * Build the fixture every scenario uses: `main` holds `_quarto.yml`, `index.qmd`, `a.qmd`, `c.qmd`, and
 * `img/logo.png`; `feature` changes `a.qmd` and `img/logo.png`, removes `c.qmd`, and adds `b.qmd`. Ends on `main`.
 */
export function buildFixtureRepo(dir: string): GitRepo & FixtureBranches {
  const repo = GitRepo.init(dir);
  const logoMain = pngBytes(10);
  const logoFeature = pngBytes(200);
  for (const [rel, content] of Object.entries(MAIN_FILES)) repo.writeFile(rel, content);
  repo.writeFile('img/logo.png', logoMain);
  repo.commitAll('main: initial');
  repo.switchCreate('feature');
  repo.writeFile('a.qmd', FEATURE_A);
  repo.writeFile('b.qmd', FEATURE_B);
  repo.writeFile('img/logo.png', logoFeature);
  run('git', ['rm', '--quiet', 'c.qmd'], dir);
  repo.commitAll('feature: change a and logo, add b, remove c');
  repo.switch('main');
  return Object.assign(repo, { logoMain, logoFeature });
}

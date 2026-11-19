/**
 * A throwaway git repository with a few committed files, for the plans that
 * need a git-enabled project (GE-9 onward). Uses the `git` on PATH, with an
 * identity set in the repo so no global configuration is needed.
 */
import { execFile } from 'node:child_process';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { promisify } from 'node:util';
import { tempDir } from './hub';

const run = promisify(execFile);

export async function git(cwd: string, ...args: string[]): Promise<string> {
  const { stdout } = await run('git', args, { cwd });
  return stdout.trim();
}

/** Create a repo on `main` with `files` (path to text) committed as one commit. */
export async function makeGitRepo(files: Record<string, string>): Promise<{ dir: string; head: string }> {
  const dir = await tempDir('ge8-git-');
  await git(dir, 'init', '-q', '-b', 'main');
  await git(dir, 'config', 'user.email', 'e2e@example.invalid');
  await git(dir, 'config', 'user.name', 'E2E');
  await git(dir, 'config', 'commit.gpgsign', 'false');
  for (const [rel, text] of Object.entries(files)) {
    await fs.mkdir(path.dirname(path.join(dir, rel)), { recursive: true });
    await fs.writeFile(path.join(dir, rel), text);
  }
  await git(dir, 'add', '-A');
  await git(dir, 'commit', '-q', '-m', 'initial');
  return { dir, head: await git(dir, 'rev-parse', 'HEAD') };
}

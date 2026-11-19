/**
 * Keep `.quarto-hub/` out of `git status` (GE-9, editor-only mode): the extension
 * writes its local storage there inside a directory a hub serves, and a repository
 * would otherwise show it as untracked. The rule goes in `.git/info/exclude`, which
 * is local to the clone and never committed.
 */
import { execFile } from 'node:child_process';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { META_DIR } from './ignore';

function git(cwd: string, args: string[]): Promise<string | null> {
  return new Promise((resolve) => {
    execFile('git', args, { cwd, windowsHide: true, timeout: 15_000, env: { ...process.env, GIT_OPTIONAL_LOCKS: '0' } }, (err, stdout) =>
      resolve(err ? null : stdout.trim()),
    );
  });
}

/** True when the rule was added; false when it was already there or `root` is not inside a git repository. */
export async function excludeMetaDir(root: string): Promise<boolean> {
  const rel = await git(root, ['rev-parse', '--git-path', 'info/exclude']);
  if (!rel) return false;
  const file = path.resolve(root, rel);
  const rule = `${META_DIR}/`;
  const current = await fs.readFile(file, 'utf8').catch(() => '');
  if (current.split(/\r?\n/).includes(rule)) return false;
  await fs.mkdir(path.dirname(file), { recursive: true });
  await fs.appendFile(file, `${current === '' || current.endsWith('\n') ? '' : '\n'}${rule}\n`);
  return true;
}

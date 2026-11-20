import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { cleanupGitTempDirs, cloneRepo, repoWith } from './git-fixtures';

// GE-17: the Windows CI runner's global git config has core.autocrlf=true. A fixture clone must not depend on it:
// the clone's files are written during `git clone`, before any per-repo config can be set.
describe('git fixtures do not depend on the global git configuration', () => {
  const saved = process.env.GIT_CONFIG_GLOBAL;
  afterEach(() => {
    if (saved === undefined) delete process.env.GIT_CONFIG_GLOBAL;
    else process.env.GIT_CONFIG_GLOBAL = saved;
    cleanupGitTempDirs();
  });

  it('cloneRepo checks files out with LF even when the global core.autocrlf is true', () => {
    const cfg = join(mkdtempSync(join(tmpdir(), 'ge17-gitcfg-')), 'gitconfig');
    writeFileSync(cfg, '[core]\n\tautocrlf = true\n');
    process.env.GIT_CONFIG_GLOBAL = cfg;
    const origin = repoWith({ 'a.qmd': 'A\n' });
    const clone = cloneRepo(origin);
    expect(readFileSync(join(clone, 'a.qmd'), 'utf8')).toBe('A\n');
  });
});

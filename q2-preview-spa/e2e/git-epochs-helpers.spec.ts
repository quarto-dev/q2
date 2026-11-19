/**
 * GE-13: checks of the git helpers the git-epochs scenarios stand on. They are test code, but a wrong helper makes
 * every scenario lie, so they get their own small specs (no browser, no server).
 */

import { test, expect } from '@playwright/test';
import { existsSync, readFileSync } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { GitError, GitRepo, buildFixtureRepo, jjAvailable } from './helpers/gitRepo';

let dir: string;
test.beforeEach(async () => {
  dir = await mkdtemp(path.join(tmpdir(), 'q2-gitrepo-check-'));
});
test.afterEach(async () => {
  await rm(dir, { recursive: true, force: true });
});

test.describe('gitRepo helper', () => {
  test('init sets a fixed author and ignores the global config', () => {
    const repo = GitRepo.init(dir);
    repo.writeFile('x.txt', 'x\n');
    repo.commitAll('first');
    expect(repo.git('log', '-1', '--format=%an <%ae>').trim()).toBe('GE Test <ge-test@example.invalid>');
    expect(repo.git('config', '--get', 'core.autocrlf').trim()).toBe('false');
  });

  test('switch, switchCreate, and resetHard move the tree', () => {
    const repo = GitRepo.init(dir);
    repo.writeFile('x.txt', 'one\n');
    repo.commitAll('one');
    repo.switchCreate('other');
    repo.writeFile('x.txt', 'two\n');
    repo.commitAll('two');
    repo.switch('main');
    expect(readFileSync(path.join(dir, 'x.txt'), 'utf8')).toBe('one\n');
    repo.writeFile('x.txt', 'dirty\n');
    repo.resetHard();
    expect(readFileSync(path.join(dir, 'x.txt'), 'utf8')).toBe('one\n');
  });

  test('a failing command throws with git\'s stderr', () => {
    const repo = GitRepo.init(dir);
    expect(() => repo.switch('no-such-branch')).toThrow(GitError);
    try {
      repo.switch('no-such-branch');
    } catch (e) {
      expect((e as GitError).stderr).toMatch(/no-such-branch/);
    }
  });

  test('markerPath resolves inside the git dir', () => {
    const repo = GitRepo.init(dir);
    expect(repo.markerPath('index.lock')).toBe(path.join(dir, '.git', 'index.lock'));
  });

  test('rebase returns stopped (no throw) on a conflict, and continue finishes it', () => {
    const repo = GitRepo.init(dir);
    repo.writeFile('f.txt', 'base\n');
    repo.commitAll('base');
    repo.switchCreate('side');
    repo.writeFile('f.txt', 'side\n');
    repo.commitAll('side');
    repo.switch('main');
    repo.writeFile('f.txt', 'main\n');
    repo.commitAll('main edit');
    repo.switch('side');
    expect(repo.rebase('main')).toEqual({ stopped: true });
    expect(repo.rebaseInProgress()).toBe(true);
    expect(existsSync(repo.markerPath('rebase-merge'))).toBe(true);
    repo.writeFile('f.txt', 'resolved\n');
    expect(repo.rebaseContinue()).toEqual({ stopped: false });
    expect(repo.rebaseInProgress()).toBe(false);
    expect(readFileSync(path.join(dir, 'f.txt'), 'utf8')).toBe('resolved\n');
  });

  test('rebaseAbort ends a stopped rebase', () => {
    const repo = GitRepo.init(dir);
    repo.writeFile('f.txt', 'base\n');
    repo.commitAll('base');
    repo.switchCreate('side');
    repo.writeFile('f.txt', 'side\n');
    repo.commitAll('side');
    repo.switch('main');
    repo.writeFile('f.txt', 'main\n');
    repo.commitAll('main edit');
    repo.switch('side');
    expect(repo.rebase('main').stopped).toBe(true);
    repo.rebaseAbort();
    expect(repo.rebaseInProgress()).toBe(false);
  });

  test('the fixture repo ends on main with the described feature branch', () => {
    const repo = buildFixtureRepo(dir);
    expect(repo.git('branch', '--show-current').trim()).toBe('main');
    const main = repo.git('ls-tree', '-r', '--name-only', 'main').trim().split('\n');
    expect(main.sort()).toEqual(['_quarto.yml', 'a.qmd', 'c.qmd', 'img/logo.png', 'index.qmd']);
    const feature = repo.git('ls-tree', '-r', '--name-only', 'feature').trim().split('\n');
    expect(feature.sort()).toEqual(['_quarto.yml', 'a.qmd', 'b.qmd', 'img/logo.png', 'index.qmd']);
    expect(repo.logoMain.equals(repo.logoFeature)).toBe(false);
    expect(repo.git('status', '--porcelain').trim()).toBe('');
  });

  test('jjAvailable is false when jj cannot run', () => {
    const saved = process.env.PATH;
    process.env.PATH = dir; // an empty directory: no jj on it
    try {
      expect(jjAvailable()).toBe(false);
    } finally {
      process.env.PATH = saved;
    }
  });
});

import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { buildJournal, JOURNAL_FILES } from '../../src/journal';

const dirs: string[] = [];
const tmp = () => {
  const d = mkdtempSync(path.join(tmpdir(), 'ge19-journal-'));
  dirs.push(d);
  return d;
};
afterAll(() => dirs.forEach((d) => rmSync(d, { recursive: true, force: true })));
const git = (cwd: string, ...a: string[]) =>
  execFileSync('git', a, { cwd, encoding: 'utf8', env: { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_SYSTEM: '/dev/null' } });
const files = (cwd: string, ref: string) => git(cwd, 'ls-tree', '-r', '--name-only', ref).split('\n').filter(Boolean).sort();

describe('journal builder (DM9)', () => {
  it('two builds give byte-identical repositories', () => {
    const a = buildJournal(tmp());
    const b = buildJournal(tmp());
    for (const br of ['main', 'feature', 'revision']) {
      expect(git(a, 'rev-parse', br).trim()).toBe(git(b, 'rev-parse', br).trim());
    }
  });

  it('main, feature, and revision hold exactly the specified files', () => {
    const d = buildJournal(tmp());
    const main = [...JOURNAL_FILES.main].sort();
    expect(files(d, 'main')).toEqual(main);
    expect(files(d, 'feature')).toEqual(
      [...main.filter((f) => f !== 'rott-mildew-2025-preliminary-fusion.qmd'), 'celia-2026-corrigendum-variance-components.qmd'].sort(),
    );
    expect(files(d, 'revision')).toEqual(main);
  });

  it('leaves main checked out and the tree clean', () => {
    const d = buildJournal(tmp());
    expect(git(d, 'branch', '--show-current').trim()).toBe('main');
    expect(git(d, 'status', '--porcelain').trim()).toBe('');
  });

  it('rebasing revision onto main stops with a conflict in the R paper abstract', () => {
    const d = buildJournal(tmp());
    git(d, 'switch', 'revision');
    // revision must diverge from main: build puts main one commit ahead of the fork point
    let failed = false;
    try {
      git(d, 'rebase', 'main');
    } catch {
      failed = true;
    }
    expect(failed).toBe(true);
    expect(git(d, 'diff', '--name-only', '--diff-filter=U').trim()).toBe('celia-2026-tip-extension-lmm.qmd');
    git(d, 'rebase', '--abort');
  });

  it('has the mycology cast and the engines config', () => {
    const d = buildJournal(tmp());
    expect(git(d, 'show', 'main:_quarto.yml')).toMatch(/engine:\s*auto/);
    expect(git(d, 'show', 'main:celia-2026-tip-extension-lmm.qmd')).toContain('Myra Celia');
    expect(git(d, 'show', 'main:morel-2026-fusion-spectra.qmd')).toContain('Maurice Morel');
    expect(git(d, 'show', 'main:.gitignore')).toContain('.venv');
  });
});

/**
 * The demo's fixture: the Journal of Theoretical Mycodynamics, vol. 17, issue 3 (DM9).
 *
 * `buildJournal(dir)` makes a deterministic git repository (fixed dates, identity, and bytes) with branches `main`,
 * `feature` (Myra's reanalysis), and `revision` (edits the abstract line that `main` also edits, for the rebase conflict).
 * Papers are data in `./journal-content`; the micrograph PNGs and the R and Python project files are committed under `../journal/`.
 */

import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { papers, indexQmd, quartoYml, GITIGNORE } from './journal-content';

const here = path.dirname(fileURLToPath(import.meta.url));
const assets = path.join(here, '..', 'journal');

const R_PAPER = 'celia-2026-tip-extension-lmm.qmd';
const PILOT = 'rott-mildew-2025-preliminary-fusion.qmd';
const CORRIGENDUM = 'celia-2026-corrigendum-variance-components.qmd';

export const JOURNAL_FILES = {
  main: [
    '.gitignore',
    '_quarto.yml',
    'index.qmd',
    'pyproject.toml',
    'uv.lock',
    'figures/micrograph.png',
    'celia-morel-2026-anastomosis-kinetics.qmd',
    'celia-2026-humidity-chamber-calibration.qmd',
    R_PAPER,
    'morel-2026-fusion-spectra.qmd',
    PILOT,
  ],
} as const;

const IDENTITY = {
  GIT_AUTHOR_NAME: 'Myra Celia',
  GIT_AUTHOR_EMAIL: 'celia@mycodynamics.example',
  GIT_COMMITTER_NAME: 'Myra Celia',
  GIT_COMMITTER_EMAIL: 'celia@mycodynamics.example',
};

export function gitEnv(date?: string): NodeJS.ProcessEnv {
  const empty = emptyConfig();
  return {
    ...process.env,
    ...IDENTITY,
    ...(date ? { GIT_AUTHOR_DATE: date, GIT_COMMITTER_DATE: date } : {}),
    GIT_CONFIG_GLOBAL: empty,
    GIT_CONFIG_SYSTEM: empty,
    GIT_CONFIG_NOSYSTEM: '1',
    GIT_TERMINAL_PROMPT: '0',
    GIT_EDITOR: 'true',
  };
}

let emptyCfg: string | undefined;
export function emptyConfig(): string {
  if (!emptyCfg) {
    const d = mkdtempSync(path.join(tmpdir(), 'ge19-gitcfg-'));
    emptyCfg = path.join(d, 'config');
    writeFileSync(emptyCfg, '');
  }
  return emptyCfg;
}

function write(dir: string, rel: string, data: string | Buffer): void {
  const full = path.join(dir, rel);
  mkdirSync(path.dirname(full), { recursive: true });
  writeFileSync(full, data);
}

/** Build the repository in `dir` (which must exist and be empty). Returns `dir`. */
export function buildJournal(dir: string): string {
  const git = (date: string | undefined, ...a: string[]) => execFileSync('git', a, { cwd: dir, env: gitEnv(date), stdio: ['ignore', 'pipe', 'pipe'] });
  const commit = (date: string, msg: string) => {
    git(date, 'add', '-A');
    git(date, 'commit', '--quiet', '-m', msg);
  };
  git(undefined, 'init', '--quiet', '--initial-branch=main');
  git(undefined, 'config', 'core.autocrlf', 'false');
  git(undefined, 'config', 'commit.gpgsign', 'false');
  git(undefined, 'config', 'rebase.autoStash', 'false');

  const base = papers('main');
  write(dir, '.gitignore', GITIGNORE);
  write(dir, '_quarto.yml', quartoYml());
  write(dir, 'index.qmd', indexQmd(Object.keys(base)));
  for (const [f, text] of Object.entries(base)) write(dir, f, text);
  copyFileSync(path.join(assets, 'pyproject.toml'), path.join(dir, 'pyproject.toml'));
  copyFileSync(path.join(assets, 'uv.lock'), path.join(dir, 'uv.lock'));
  mkdirSync(path.join(dir, 'figures'), { recursive: true });
  copyFileSync(path.join(assets, 'micrograph-main.png'), path.join(dir, 'figures/micrograph.png'));
  commit('2026-03-02T09:00:00Z', 'Issue 17(3): the papers');

  // revision forks here and rewrites the abstract line main rewrites next.
  git(undefined, 'branch', 'revision');
  write(dir, R_PAPER, papers('main-edit')[R_PAPER]!);
  commit('2026-03-09T09:00:00Z', 'Celia: tighten the abstract');

  git(undefined, 'switch', '--quiet', 'revision');
  write(dir, R_PAPER, papers('revision')[R_PAPER]!);
  commit('2026-03-10T09:00:00Z', 'Reviewer 2: soften the abstract');

  git(undefined, 'switch', '--quiet', '-c', 'feature', 'main');
  const feat = papers('feature');
  write(dir, R_PAPER, feat[R_PAPER]!);
  write(dir, 'morel-2026-fusion-spectra.qmd', feat['morel-2026-fusion-spectra.qmd']!);
  copyFileSync(path.join(assets, 'micrograph-feature.png'), path.join(dir, 'figures/micrograph.png'));
  git(undefined, 'rm', '--quiet', PILOT);
  write(dir, CORRIGENDUM, feat[CORRIGENDUM]!);
  write(dir, 'index.qmd', indexQmd(Object.keys(feat)));
  commit('2026-03-16T09:00:00Z', 'Reanalysis: new model, recolored micrograph, pilot study retracted, corrigendum');

  git(undefined, 'switch', '--quiet', 'main');
  return dir;
}


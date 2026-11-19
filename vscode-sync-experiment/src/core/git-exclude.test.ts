import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { excludeMetaDir } from './git-exclude';
import { tempDir } from './test-support';

let dir: string;
beforeEach(async () => {
  dir = await tempDir('ge9-excl-');
});
afterEach(async () => {
  await fs.rm(dir, { recursive: true, force: true });
});

const git = (...args: string[]) => execFileSync('git', args, { cwd: dir, encoding: 'utf8' });

describe('excludeMetaDir', () => {
  it('adds .quarto-hub/ to .git/info/exclude once, so git status ignores it', async () => {
    git('init', '-q');
    await fs.mkdir(path.join(dir, '.quarto-hub'));
    await fs.writeFile(path.join(dir, '.quarto-hub', 'project.json'), '{}');
    expect(git('status', '--porcelain')).toContain('.quarto-hub');
    expect(await excludeMetaDir(dir)).toBe(true);
    expect(await excludeMetaDir(dir)).toBe(false); // already there
    expect(git('status', '--porcelain')).not.toContain('.quarto-hub');
    const exclude = await fs.readFile(path.join(dir, '.git', 'info', 'exclude'), 'utf8');
    expect(exclude.split('\n').filter((l) => l === '.quarto-hub/')).toHaveLength(1);
  });

  it('does nothing outside a git repository', async () => {
    expect(await excludeMetaDir(dir)).toBe(false);
    expect(await fs.readdir(dir)).toEqual([]);
  });

  it('works for a project below the repository root', async () => {
    git('init', '-q');
    await fs.mkdir(path.join(dir, 'site'));
    expect(await excludeMetaDir(path.join(dir, 'site'))).toBe(true);
    expect(await fs.readFile(path.join(dir, '.git', 'info', 'exclude'), 'utf8')).toContain('.quarto-hub/');
  });
});

import { describe, expect, it } from 'vitest';
import { isIgnoredPath } from './ignore';

describe('isIgnoredPath (G23)', () => {
  const ignored: [string, string][] = [
    ['empty path', ''],
    ['.git at the top', '.git/config'],
    ['.git at depth', 'a/b/.git/config'],
    ['the .git entry itself', 'sub/.git'],
    ['.quarto-hub at the top', '.quarto-hub/project.json'],
    ['.quarto-hub at depth', 'sub/.quarto-hub/automerge/x'],
    ['a temp folder named like .quarto-hub', '.quarto-hub-tmp-abc/x'],
    ['.DS_Store at the top', '.DS_Store'],
    ['.DS_Store at depth', 'a/b/.DS_Store'],
    ['.vscode', '.vscode/settings.json'],
    ['.quarto', '.quarto/x'],
    ['_site', '_site/index.html'],
    ['_book', '_book/index.html'],
    ['_freeze', '_freeze/x/y.json'],
    ['node_modules', 'node_modules/x/index.js'],
    ['a Windows-style .git path', 'a\\b\\.git\\x'],
    ['a Windows-style top-level entry', '_site\\index.html'],
  ];
  it.each(ignored)('ignores %s', (_name, rel) => {
    expect(isIgnoredPath(rel)).toBe(true);
  });

  const kept: [string, string][] = [
    ['a plain file', 'index.qmd'],
    ['a nested file', 'docs/guide/a.qmd'],
    ['a dotfile that is not special', '.gitignore'],
    ['a name that only contains .git', 'a/x.git/y'],
    ['a name that only starts like .quarto', 'a/.quarto-notes/y'],
    ['_site below the top level', 'docs/_site/a.html'],
    ['a Windows-style plain path', 'a\\b.qmd'],
  ];
  it.each(kept)('keeps %s', (_name, rel) => {
    expect(isIgnoredPath(rel)).toBe(false);
  });

  it('skips a bound subfolder as a whole, and only that subfolder', () => {
    const bound = new Set(['mirror', 'deep/other']);
    expect(isIgnoredPath('mirror', bound)).toBe(true);
    expect(isIgnoredPath('mirror/a.qmd', bound)).toBe(true);
    expect(isIgnoredPath('deep/other/x/y.qmd', bound)).toBe(true);
    expect(isIgnoredPath('mirror2/a.qmd', bound)).toBe(false);
    expect(isIgnoredPath('deep/a.qmd', bound)).toBe(false);
    expect(isIgnoredPath('a.qmd', bound)).toBe(false);
  });

  it('normalizes a Windows-style bound subfolder lookup', () => {
    expect(isIgnoredPath('mirror\\a.qmd', new Set(['mirror']))).toBe(true);
  });
});

/**
 * Unit tests for file tree utilities
 */

import { describe, it, expect } from 'vitest';
import {
  buildFileTree,
  getAncestorPaths,
  computeExpandedFolders,
  type FileTreeNode,
} from './fileTree';

describe('buildFileTree', () => {
  it('returns empty root for empty file list', () => {
    const tree = buildFileTree([]);
    expect(tree.children).toHaveLength(0);
    expect(tree.type).toBe('folder');
    expect(tree.name).toBe('');
    expect(tree.path).toBe('');
  });

  it('places root-level files directly under root', () => {
    const files = [
      { path: 'index.qmd', docId: '1' },
      { path: 'README.md', docId: '2' },
    ];
    const tree = buildFileTree(files);
    expect(tree.children).toHaveLength(2);
    expect(tree.children.every((c) => c.type === 'file')).toBe(true);
  });

  it('creates single-level folder structure', () => {
    const files = [
      { path: 'images/foo.png', docId: '1' },
      { path: 'images/bar.png', docId: '2' },
    ];
    const tree = buildFileTree(files);
    expect(tree.children).toHaveLength(1);
    expect(tree.children[0].name).toBe('images');
    expect(tree.children[0].type).toBe('folder');
    expect(tree.children[0].children).toHaveLength(2);
  });

  it('creates nested folder structure', () => {
    const files = [{ path: 'src/components/ui/Button.tsx', docId: '1' }];
    const tree = buildFileTree(files);

    // src folder
    expect(tree.children).toHaveLength(1);
    const src = tree.children[0];
    expect(src.name).toBe('src');
    expect(src.path).toBe('src');
    expect(src.type).toBe('folder');

    // components folder
    expect(src.children).toHaveLength(1);
    const components = src.children[0];
    expect(components.name).toBe('components');
    expect(components.path).toBe('src/components');
    expect(components.type).toBe('folder');

    // ui folder
    expect(components.children).toHaveLength(1);
    const ui = components.children[0];
    expect(ui.name).toBe('ui');
    expect(ui.path).toBe('src/components/ui');
    expect(ui.type).toBe('folder');

    // Button.tsx file
    expect(ui.children).toHaveLength(1);
    expect(ui.children[0].name).toBe('Button.tsx');
    expect(ui.children[0].type).toBe('file');
    expect(ui.children[0].path).toBe('src/components/ui/Button.tsx');
  });

  it('handles mixed depths correctly', () => {
    const files = [
      { path: 'index.qmd', docId: '1' },
      { path: 'src/main.ts', docId: '2' },
      { path: 'src/components/App.tsx', docId: '3' },
    ];
    const tree = buildFileTree(files);

    // Root should have: src (folder) and index.qmd (file)
    // Folders come before files, so src first
    expect(tree.children).toHaveLength(2);
    expect(tree.children[0].name).toBe('src');
    expect(tree.children[0].type).toBe('folder');
    expect(tree.children[1].name).toBe('index.qmd');
    expect(tree.children[1].type).toBe('file');

    // src folder should have: components (folder) and main.ts (file)
    const src = tree.children[0];
    expect(src.children).toHaveLength(2);
    expect(src.children[0].name).toBe('components');
    expect(src.children[0].type).toBe('folder');
    expect(src.children[1].name).toBe('main.ts');
    expect(src.children[1].type).toBe('file');
  });

  it('sorts folders before files at each level', () => {
    const files = [
      { path: 'zebra.txt', docId: '1' },
      { path: 'alpha/file.txt', docId: '2' },
      { path: 'beta.txt', docId: '3' },
    ];
    const tree = buildFileTree(files);

    // Order should be: alpha (folder), beta.txt (file), zebra.txt (file)
    expect(tree.children).toHaveLength(3);
    expect(tree.children[0].name).toBe('alpha');
    expect(tree.children[0].type).toBe('folder');
    expect(tree.children[1].name).toBe('beta.txt');
    expect(tree.children[1].type).toBe('file');
    expect(tree.children[2].name).toBe('zebra.txt');
    expect(tree.children[2].type).toBe('file');
  });

  it('sorts alphabetically within folders and files', () => {
    const files = [
      { path: 'c.txt', docId: '1' },
      { path: 'a.txt', docId: '2' },
      { path: 'b.txt', docId: '3' },
    ];
    const tree = buildFileTree(files);

    expect(tree.children[0].name).toBe('a.txt');
    expect(tree.children[1].name).toBe('b.txt');
    expect(tree.children[2].name).toBe('c.txt');
  });

  it('sorts folders alphabetically', () => {
    const files = [
      { path: 'zeta/file.txt', docId: '1' },
      { path: 'alpha/file.txt', docId: '2' },
      { path: 'gamma/file.txt', docId: '3' },
    ];
    const tree = buildFileTree(files);

    expect(tree.children[0].name).toBe('alpha');
    expect(tree.children[1].name).toBe('gamma');
    expect(tree.children[2].name).toBe('zeta');
  });

  it('preserves FileEntry reference in file nodes', () => {
    const file = { path: 'test.txt', docId: 'doc-123' };
    const tree = buildFileTree([file]);

    expect(tree.children[0].file).toBe(file);
  });

  it('handles multiple files in same nested folder', () => {
    const files = [
      { path: 'src/utils/a.ts', docId: '1' },
      { path: 'src/utils/b.ts', docId: '2' },
      { path: 'src/utils/c.ts', docId: '3' },
    ];
    const tree = buildFileTree(files);

    const src = tree.children[0];
    const utils = src.children[0];

    expect(utils.children).toHaveLength(3);
    expect(utils.children[0].name).toBe('a.ts');
    expect(utils.children[1].name).toBe('b.ts');
    expect(utils.children[2].name).toBe('c.ts');
  });

  it('handles sibling folders at same level', () => {
    const files = [
      { path: 'src/components/Button.tsx', docId: '1' },
      { path: 'src/utils/helpers.ts', docId: '2' },
    ];
    const tree = buildFileTree(files);

    const src = tree.children[0];
    expect(src.children).toHaveLength(2);
    expect(src.children[0].name).toBe('components');
    expect(src.children[1].name).toBe('utils');
  });
});

describe('getAncestorPaths', () => {
  it('returns empty array for root-level file', () => {
    expect(getAncestorPaths('index.qmd')).toEqual([]);
  });

  it('returns single ancestor for one-level nesting', () => {
    expect(getAncestorPaths('src/main.ts')).toEqual(['src']);
  });

  it('returns all ancestors for deep nesting', () => {
    expect(getAncestorPaths('src/components/ui/Button.tsx')).toEqual([
      'src',
      'src/components',
      'src/components/ui',
    ]);
  });

  it('handles two-level nesting', () => {
    expect(getAncestorPaths('images/icons/logo.png')).toEqual([
      'images',
      'images/icons',
    ]);
  });
});

describe('computeExpandedFolders', () => {
  it('returns existing set when no file selected', () => {
    const existing = new Set(['foo']);
    const result = computeExpandedFolders(existing, null);
    expect(result).toBe(existing); // Same reference
  });

  it('adds ancestor paths to existing expanded set', () => {
    const existing = new Set(['other']);
    const result = computeExpandedFolders(existing, 'src/components/Button.tsx');

    expect(result.has('other')).toBe(true);
    expect(result.has('src')).toBe(true);
    expect(result.has('src/components')).toBe(true);
    expect(result.size).toBe(3);
  });

  it('does not modify original set', () => {
    const existing = new Set(['foo']);
    const result = computeExpandedFolders(existing, 'src/main.ts');

    expect(existing.size).toBe(1);
    expect(existing.has('src')).toBe(false);
    expect(result.size).toBe(2);
    expect(result.has('src')).toBe(true);
  });

  it('handles root-level file (no ancestors)', () => {
    const existing = new Set(['foo']);
    const result = computeExpandedFolders(existing, 'index.qmd');
    // No ancestors to add, but still returns same set since nothing changed
    expect(result).toBe(existing);
  });

  it('returns same set if all ancestors already expanded', () => {
    const existing = new Set(['src', 'src/components']);
    const result = computeExpandedFolders(existing, 'src/components/Button.tsx');
    // All ancestors are already in the set
    expect(result).toBe(existing);
  });

  it('handles empty initial set', () => {
    const existing = new Set<string>();
    const result = computeExpandedFolders(existing, 'a/b/c/file.txt');

    expect(result.has('a')).toBe(true);
    expect(result.has('a/b')).toBe(true);
    expect(result.has('a/b/c')).toBe(true);
    expect(result.size).toBe(3);
  });

  it('handles single-level path', () => {
    const existing = new Set<string>();
    const result = computeExpandedFolders(existing, 'folder/file.txt');

    expect(result.has('folder')).toBe(true);
    expect(result.size).toBe(1);
  });
});

import { describe, it, expect } from 'vitest';
import { pickInitialPage, type FileLike } from './pickInitialPage';

const files: FileLike[] = [
  { path: '_quarto.yml' },
  { path: 'posts/intro.qmd' },
  { path: 'about.qmd' },
  { path: 'index.qmd' },
];

describe('pickInitialPage', () => {
  it('returns the queried page when it is in the file index', () => {
    expect(pickInitialPage('?page=posts/intro.qmd', files)).toBe(
      'posts/intro.qmd',
    );
  });

  it('decodes percent-encoded path segments', () => {
    const withSpace: FileLike[] = [{ path: 'a b.qmd' }];
    // URLSearchParams takes care of decoding %20 -> space.
    expect(pickInitialPage('?page=a%20b.qmd', withSpace)).toBe('a b.qmd');
  });

  it('falls through to the first .qmd when query is missing', () => {
    expect(pickInitialPage('', files)).toBe('posts/intro.qmd');
    expect(pickInitialPage('?', files)).toBe('posts/intro.qmd');
  });

  it('falls through when the queried page is not in the index', () => {
    // `nonexistent.qmd` not in files → silent fallback to first .qmd.
    expect(pickInitialPage('?page=nonexistent.qmd', files)).toBe(
      'posts/intro.qmd',
    );
  });

  it('falls through when query is unrelated', () => {
    expect(pickInitialPage('?other=value', files)).toBe('posts/intro.qmd');
  });

  it('rejects path-traversal attempts', () => {
    // A hand-crafted URL with `..` segments must not be honored even
    // if (somehow) it matches a file path. We never want to chase a
    // user-controlled path that could read outside the project.
    expect(pickInitialPage('?page=../../etc/passwd', files)).toBe(
      'posts/intro.qmd',
    );
    expect(pickInitialPage('?page=posts/../about.qmd', files)).toBe(
      'posts/intro.qmd',
    );
  });

  it('returns null when no .qmd files are in the index', () => {
    expect(pickInitialPage('', [{ path: '_quarto.yml' }])).toBeNull();
  });

  it('treats `?page=` (empty value) as missing', () => {
    expect(pickInitialPage('?page=', files)).toBe('posts/intro.qmd');
    expect(pickInitialPage('?page=   ', files)).toBe('posts/intro.qmd');
  });

  // ─── bd-6d2wj4zp Phase 5: .md pages ───────────────────────────────

  it('honors a ?page= hint naming a .md file', () => {
    const mdFiles: FileLike[] = [{ path: 'index.qmd' }, { path: 'admin/index.md' }];
    expect(pickInitialPage('?page=admin/index.md', mdFiles)).toBe('admin/index.md');
  });

  it('falls back to the first .md only when no .qmd exists', () => {
    // .qmd stays the preferred fallback: with extension-based sync,
    // the index may contain never-rendered .md (a README) — a .qmd,
    // if present, is always deliberate content.
    const mixed: FileLike[] = [
      { path: 'README.md' },
      { path: 'about.qmd' },
    ];
    expect(pickInitialPage('', mixed)).toBe('about.qmd');

    const mdOnly: FileLike[] = [{ path: '_quarto.yml' }, { path: 'guide.md' }];
    expect(pickInitialPage('', mdOnly)).toBe('guide.md');
  });
});

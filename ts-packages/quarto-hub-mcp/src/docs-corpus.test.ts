/**
 * docs corpus primitives (CAP-13): llms-full.txt parsing, ranked search,
 * and page lookup. Pure functions over the corpus text — the subprocess
 * seam (`q2 docs llms --full`, bd-dn81ol95/bd-b6cocsxw) is tested at the
 * tool level in docs.test.ts.
 */

import { describe, it, expect } from 'vitest';

import {
  parseFullCorpus,
  searchDocs,
  findPage,
  type DocsPage,
} from './docs-tool.js';

const CORPUS = [
  '---',
  'title: Authoring Quarto Documents',
  'url: guides/authoring/index.md',
  '---',
  '',
  '# Authoring Quarto Documents',
  '',
  'Write qmd files with YAML front matter. Figures get captions with fig-cap.',
  '',
  '---',
  'title: Figures',
  'url: guides/authoring/figures.md',
  '---',
  '',
  '# Figures',
  '',
  'Cross-reference a figure with @fig-plot. Set fig-cap for the caption text.',
  'Use fig-alt for accessibility text on every image.',
  '',
  '---',
  'title: Creating projects with q2 create',
  'url: guides/projects/create.md',
  '---',
  '',
  '# Creating projects',
  '',
  'Run q2 create to scaffold a project with a _quarto.yml.',
  '',
].join('\n');

const PAGES: DocsPage[] = [
  { href: 'guides/authoring/index.md', title: 'Authoring Quarto Documents', content: '# Authoring Quarto Documents\n\nWrite qmd files with YAML front matter. Figures get captions with fig-cap.' },
  { href: 'guides/authoring/figures.md', title: 'Figures', content: '# Figures\n\nCross-reference a figure with @fig-plot. Set fig-cap for the caption text.\nUse fig-alt for accessibility text on every image.' },
  { href: 'guides/projects/create.md', title: 'Creating projects with q2 create', content: '# Creating projects\n\nRun q2 create to scaffold a project with a _quarto.yml.' },
];

describe('parseFullCorpus', () => {
  it('splits the corpus into pages on the --- header blocks', () => {
    const pages = parseFullCorpus(CORPUS);
    expect(pages).toHaveLength(3);
    expect(pages[0]).toEqual({
      href: 'guides/authoring/index.md',
      title: 'Authoring Quarto Documents',
      content: '# Authoring Quarto Documents\n\nWrite qmd files with YAML front matter. Figures get captions with fig-cap.',
    });
    expect(pages[2]!.title).toBe('Creating projects with q2 create');
  });

  it('ignores a preamble before the first page marker', () => {
    const pages = parseFullCorpus('# Site preamble\n\n' + CORPUS);
    expect(pages).toHaveLength(3);
    expect(pages[0]!.title).toBe('Authoring Quarto Documents');
  });

  it('a page body containing a YAML-looking --- fence inside a code block is not split', () => {
    const corpus =
      '---\ntitle: A\nurl: a.md\n---\n\n# A\n\n```yaml\n---\nfoo: bar\n---\n```\n\n---\ntitle: B\nurl: b.md\n---\n\n# B\n';
    const pages = parseFullCorpus(corpus);
    expect(pages).toHaveLength(2);
    expect(pages[0]!.content).toContain('foo: bar');
  });
});

describe('searchDocs', () => {
  it('ranks title matches above content matches', () => {
    const results = searchDocs('figures', PAGES);
    expect(results.length).toBeGreaterThan(0);
    expect(results[0]!.href).toBe('guides/authoring/figures.md');
  });

  it('is case-insensitive', () => {
    const results = searchDocs('FIG-CAP', PAGES);
    expect(results.length).toBe(2);
  });

  it('carries a content snippet for body matches', () => {
    const results = searchDocs('accessibility', PAGES);
    expect(results).toHaveLength(1);
    expect(results[0]!.snippet).toMatch(/accessibility/);
  });

  it('returns every hit in rank order (the caller slices for its cap)', () => {
    expect(searchDocs('zzz-no-match', PAGES)).toEqual([]);
    const all = searchDocs('fig', PAGES);
    expect(all.length).toBe(2);
    expect(all[0]!.href).toBe('guides/authoring/figures.md');
  });

  it('scores multiple content hits on one page above a single hit', () => {
    const pages: DocsPage[] = [
      { href: 'a.md', title: 'A', content: 'alpha beta gamma' },
      { href: 'b.md', title: 'B', content: 'alpha\nalpha\nalpha' },
    ];
    const results = searchDocs('alpha', pages);
    expect(results[0]!.href).toBe('b.md');
  });
});

describe('findPage', () => {
  it('fetches by exact href', () => {
    const found = findPage(PAGES, 'guides/authoring/figures.md');
    expect(found.ok).toBe(true);
    if (found.ok) expect(found.page.title).toBe('Figures');
  });

  it('accepts the .qmd spelling, extensionless, and directory-index forms', () => {
    expect(findPage(PAGES, 'guides/authoring/figures.qmd').ok).toBe(true);
    expect(findPage(PAGES, 'guides/authoring/figures').ok).toBe(true);
    const idx = findPage(PAGES, 'guides/authoring');
    expect(idx.ok).toBe(true);
    if (idx.ok) expect(idx.page.href).toBe('guides/authoring/index.md');
  });

  it('a miss lists nearby hrefs', () => {
    const miss = findPage(PAGES, 'guides/authoring/figure.md');
    expect(miss.ok).toBe(false);
    if (!miss.ok) {
      expect(miss.suggestions).toContain('guides/authoring/figures.md');
    }
  });
});

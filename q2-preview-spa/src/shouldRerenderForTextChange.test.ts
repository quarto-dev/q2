/**
 * Unit tests for `shouldRerenderForTextChange` (preview VFS contract D11).
 *
 * The render reports what it read, probed and listed; a text change
 * re-renders the active page when the render depended on it. Before the
 * first render answers (or on a PDF page, which the server compiles) every
 * change passes. This replaced the Phase D.6 filter on the
 * `/api/preview/deps` single-hop include set.
 */

import { describe, it, expect } from 'vitest';
import { shouldRerenderForTextChange } from './PreviewApp';

const readSet = {
  page: 'index.qmd',
  paths: new Set(['index.qmd', '_quarto.yml', 'refs.bib', 'included.qmd', 'partials/_vars.scss']),
  dirs: new Set(['assets']),
};

describe('shouldRerenderForTextChange', () => {
  it('always re-renders when there is no active file', () => {
    expect(shouldRerenderForTextChange('anything.md', null, readSet)).toBe(true);
  });

  it('fails open when no read set has arrived yet', () => {
    expect(shouldRerenderForTextChange('other.qmd', 'index.qmd', null)).toBe(true);
    expect(shouldRerenderForTextChange('README.md', 'index.qmd', null)).toBe(true);
  });

  it('fails open when the read set belongs to another page', () => {
    expect(shouldRerenderForTextChange('other.css', 'about.qmd', readSet)).toBe(true);
  });

  it('re-renders for a path the render read or probed', () => {
    expect(shouldRerenderForTextChange('index.qmd', 'index.qmd', readSet)).toBe(true);
    expect(shouldRerenderForTextChange('included.qmd', 'index.qmd', readSet)).toBe(true);
    expect(shouldRerenderForTextChange('refs.bib', 'index.qmd', readSet)).toBe(true);
    expect(shouldRerenderForTextChange('_quarto.yml', 'index.qmd', readSet)).toBe(true);
    // Probed and missed, now created.
    expect(shouldRerenderForTextChange('partials/_vars.scss', 'index.qmd', readSet, true)).toBe(true);
  });

  it('re-renders for a file created in a directory the render listed, not an edit there', () => {
    expect(shouldRerenderForTextChange('assets/new.css', 'index.qmd', readSet, true)).toBe(true);
    expect(shouldRerenderForTextChange('assets/old.css', 'index.qmd', readSet, false)).toBe(false);
  });

  it('ignores an unrelated sibling, whatever its type', () => {
    expect(shouldRerenderForTextChange('unrelated.qmd', 'index.qmd', readSet)).toBe(false);
    expect(shouldRerenderForTextChange('README.md', 'index.qmd', readSet)).toBe(false);
    expect(shouldRerenderForTextChange('other.css', 'index.qmd', readSet)).toBe(false);
    expect(shouldRerenderForTextChange('data.csv', 'index.qmd', readSet)).toBe(false);
  });

  it('always passes custom components, which the main thread compiles', () => {
    expect(shouldRerenderForTextChange('Widget.tsx', 'index.qmd', readSet)).toBe(true);
    expect(shouldRerenderForTextChange('ui/Button.TSX', 'index.qmd', readSet)).toBe(true);
  });
});

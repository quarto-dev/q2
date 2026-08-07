/**
 * Unit tests for `shouldRerenderForTextChange` (Phase D.6, bd-kw93.12;
 * `.md` dep-filtering added in bd-6d2wj4zp Phase 5).
 *
 * With hub-wide extension-based sync (D10), *every* `.md` in the project
 * syncs and watches — including never-rendered ones (README, notes). The
 * dep filter is what keeps those edits from re-rendering the active page:
 * source files (`.qmd` AND `.md`) only trigger a re-render when they are
 * in the active page's dep set; non-source files (config, CSS, .tsx)
 * remain project-wide signals that always pass.
 */

import { describe, it, expect } from 'vitest';
import { shouldRerenderForTextChange } from './PreviewApp';

describe('shouldRerenderForTextChange', () => {
  const deps = new Set(['included.qmd', 'snippets/part.md']);

  it('always re-renders when there is no active file', () => {
    expect(shouldRerenderForTextChange('anything.md', null, deps)).toBe(true);
  });

  it('fails open when the dep set has not arrived yet', () => {
    expect(shouldRerenderForTextChange('other.qmd', 'index.qmd', null)).toBe(true);
    expect(shouldRerenderForTextChange('other.md', 'index.qmd', null)).toBe(true);
  });

  it('passes non-source project-wide signals through', () => {
    expect(shouldRerenderForTextChange('_quarto.yml', 'index.qmd', deps)).toBe(true);
    expect(shouldRerenderForTextChange('styles.css', 'index.qmd', deps)).toBe(true);
    expect(shouldRerenderForTextChange('Widget.tsx', 'index.qmd', deps)).toBe(true);
  });

  it('filters .qmd edits by the dep set (pre-existing behavior)', () => {
    expect(shouldRerenderForTextChange('included.qmd', 'index.qmd', deps)).toBe(true);
    expect(shouldRerenderForTextChange('unrelated.qmd', 'index.qmd', deps)).toBe(false);
  });

  it('filters .md edits by the dep set (bd-6d2wj4zp Phase 5)', () => {
    // An unrelated .md (README, notes) must NOT re-render the active page…
    expect(shouldRerenderForTextChange('README.md', 'index.qmd', deps)).toBe(false);
    // …but a .md the active page includes must.
    expect(shouldRerenderForTextChange('snippets/part.md', 'index.qmd', deps)).toBe(true);
  });

  it('re-renders when the active .md page itself is edited', () => {
    // The deps fetch always seeds the set with the active page itself
    // (PreviewApp's "edit my own page" convention) — mirror that shape.
    const mdDeps = new Set(['admin/index.md', 'snippets/part.md']);
    expect(
      shouldRerenderForTextChange('admin/index.md', 'admin/index.md', mdDeps),
    ).toBe(true);
  });

  it('is case-insensitive on the extension', () => {
    expect(shouldRerenderForTextChange('NOTES.MD', 'index.qmd', deps)).toBe(false);
    expect(shouldRerenderForTextChange('OTHER.QMD', 'index.qmd', deps)).toBe(false);
  });
});

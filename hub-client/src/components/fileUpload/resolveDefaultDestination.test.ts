/**
 * Tests for resolveDefaultDestination
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect } from 'vitest';
import { resolveDefaultDestination, FOLDER_PATH_ATTR } from './resolveDefaultDestination';

/**
 * Build a DOM node with the given `data-folder-path` ancestry for testing.
 * The innermost returned element is the "target" — ancestors are parent nodes.
 */
function makeTarget(folderPaths: (string | null)[]): HTMLElement {
  // folderPaths[0] is outermost; folderPaths[last] is the target itself.
  let current: HTMLElement | null = null;
  for (const fp of folderPaths) {
    const el = document.createElement('div');
    if (fp !== null) {
      el.setAttribute(FOLDER_PATH_ATTR, fp);
    }
    if (current) {
      current.appendChild(el);
    }
    current = el;
  }
  return current!;
}

describe('resolveDefaultDestination', () => {
  it('returns root ("") when neither dropTarget nor selection is given', () => {
    expect(resolveDefaultDestination({})).toBe('');
  });

  describe('drop target', () => {
    it('reads data-folder-path directly from the target', () => {
      const target = makeTarget(['images']);
      expect(resolveDefaultDestination({ dropTarget: target })).toBe('images');
    });

    it('walks up to the nearest ancestor with data-folder-path', () => {
      const target = makeTarget(['images', null, null]);
      expect(resolveDefaultDestination({ dropTarget: target })).toBe('images');
    });

    it('picks the nearest ancestor when multiple ancestors are tagged', () => {
      const target = makeTarget(['outer', 'images', null]);
      expect(resolveDefaultDestination({ dropTarget: target })).toBe('images');
    });

    it('returns the empty-string folder-path (project root) if tagged as such', () => {
      const target = makeTarget(['']);
      expect(resolveDefaultDestination({ dropTarget: target })).toBe('');
    });

    it('falls back to selection if no ancestor has the attribute', () => {
      const target = makeTarget([null, null]);
      expect(
        resolveDefaultDestination({ dropTarget: target, selection: 'notes/foo.qmd' })
      ).toBe('notes');
    });

    it('falls back to root if no ancestor has the attribute and no selection', () => {
      const target = makeTarget([null, null]);
      expect(resolveDefaultDestination({ dropTarget: target })).toBe('');
    });
  });

  describe('selection fallback', () => {
    it('uses the parent folder of a selected file', () => {
      expect(resolveDefaultDestination({ selection: 'notes/foo.qmd' })).toBe('notes');
    });

    it('uses root for a file at project root', () => {
      expect(resolveDefaultDestination({ selection: 'index.qmd' })).toBe('');
    });

    it('handles deeply nested selection', () => {
      expect(
        resolveDefaultDestination({ selection: '_quarto/grammars/toml/toml.scm' })
      ).toBe('_quarto/grammars/toml');
    });

    it('returns root when selection is null', () => {
      expect(resolveDefaultDestination({ selection: null })).toBe('');
    });

    it('returns root when selection is undefined', () => {
      expect(resolveDefaultDestination({})).toBe('');
    });
  });
});

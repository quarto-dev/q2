/**
 * Tests for buildDropMarkdown
 */

import { describe, it, expect } from 'vitest';
import { buildDropMarkdown } from './dropMarkdown';

describe('buildDropMarkdown', () => {
  describe('image markdown', () => {
    it('uses the bare filename when the target is next to the current file', () => {
      expect(buildDropMarkdown('image', 'posts/hello.qmd', 'posts/photo.png')).toBe(
        '![](photo.png)'
      );
    });

    it('walks up when the target is at the project root and the file is not', () => {
      expect(buildDropMarkdown('image', 'posts/hello.qmd', 'photo.png')).toBe(
        '![](../photo.png)'
      );
    });

    it('walks up and down for a sibling directory', () => {
      expect(buildDropMarkdown('image', 'posts/hello.qmd', 'images/photo.png')).toBe(
        '![](../images/photo.png)'
      );
    });

    it('uses the bare filename when both are at the project root', () => {
      expect(buildDropMarkdown('image', 'hello.qmd', 'photo.png')).toBe(
        '![](photo.png)'
      );
    });

    it('preserves a conflict-renamed (hash-suffixed) filename', () => {
      expect(
        buildDropMarkdown('image', 'posts/hello.qmd', 'posts/photo-1a2b3c4d.png')
      ).toBe('![](photo-1a2b3c4d.png)');
    });

    it('falls back to the target path verbatim without a current file', () => {
      expect(buildDropMarkdown('image', null, 'photo.png')).toBe('![](photo.png)');
    });
  });

  describe('link markdown', () => {
    it('uses the target filename as link text and a relative path as target', () => {
      expect(buildDropMarkdown('link', 'posts/hello.qmd', 'notes.qmd')).toBe(
        '[notes.qmd](../notes.qmd)'
      );
    });

    it('links to a file in the same directory with a bare filename', () => {
      expect(buildDropMarkdown('link', 'posts/hello.qmd', 'posts/notes.qmd')).toBe(
        '[notes.qmd](notes.qmd)'
      );
    });

    it('falls back to the target path verbatim without a current file', () => {
      expect(buildDropMarkdown('link', null, 'guides/notes.qmd')).toBe(
        '[notes.qmd](guides/notes.qmd)'
      );
    });
  });
});

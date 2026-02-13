/**
 * Tests for resourceService utility functions
 */

import { describe, it, expect } from 'vitest';
import { sanitizeFilename } from './resourceService';

describe('sanitizeFilename', () => {
  describe('whitespace sanitization', () => {
    it('should replace ASCII spaces with hyphens', () => {
      expect(sanitizeFilename('my file.png')).toBe('my-file.png');
    });

    it('should replace tabs with hyphens', () => {
      expect(sanitizeFilename('my\tfile.png')).toBe('my-file.png');
    });

    it('should replace non-breaking spaces with hyphens', () => {
      expect(sanitizeFilename('my\u00A0file.png')).toBe('my-file.png');
    });

    it('should collapse multiple consecutive spaces into a single hyphen', () => {
      expect(sanitizeFilename('my   file.png')).toBe('my-file.png');
    });

    it('should collapse mixed whitespace types into a single hyphen', () => {
      expect(sanitizeFilename('my \t\u00A0file.png')).toBe('my-file.png');
    });

    it('should trim leading and trailing whitespace', () => {
      expect(sanitizeFilename(' file.png ')).toBe('file.png');
    });

    it('should handle em space (U+2003)', () => {
      expect(sanitizeFilename('my\u2003file.png')).toBe('my-file.png');
    });

    it('should handle ideographic space (U+3000)', () => {
      expect(sanitizeFilename('my\u3000file.png')).toBe('my-file.png');
    });
  });

  describe('interior dot sanitization', () => {
    it('should replace interior dots with hyphens', () => {
      expect(sanitizeFilename('my.cool.photo.jpeg')).toBe('my-cool-photo.jpeg');
    });

    it('should preserve the last dot (extension separator)', () => {
      expect(sanitizeFilename('file.png')).toBe('file.png');
    });

    it('should handle no extension (no dots)', () => {
      expect(sanitizeFilename('Makefile')).toBe('Makefile');
    });

    it('should preserve dotfiles', () => {
      expect(sanitizeFilename('.gitignore')).toBe('.gitignore');
    });
  });

  describe('combined sanitization (macOS screenshots)', () => {
    it('should sanitize a typical macOS screenshot filename', () => {
      expect(sanitizeFilename('Screenshot 2026-02-13 at 3.54.46 PM.png'))
        .toBe('Screenshot-2026-02-13-at-3-54-46-PM.png');
    });
  });

  describe('no-op cases', () => {
    it('should not modify a clean filename', () => {
      expect(sanitizeFilename('myfile.png')).toBe('myfile.png');
    });

    it('should not modify a filename with only hyphens', () => {
      expect(sanitizeFilename('my-file-name.png')).toBe('my-file-name.png');
    });
  });

  describe('hyphen collapsing', () => {
    it('should collapse consecutive hyphens from combined replacements', () => {
      // A dot followed by a space: both get replaced, should collapse
      expect(sanitizeFilename('file. name.png')).toBe('file-name.png');
    });

    it('should not leave leading hyphens after trimming whitespace', () => {
      expect(sanitizeFilename('  .file.png')).toBe('.file.png');
    });
  });
});

/**
 * Tests for wasmRenderer utility functions.
 *
 * Note: These tests focus on pure functions that don't require WASM initialization.
 * Full integration tests with WASM would require additional setup.
 */

import { describe, it, expect } from 'vitest';
import { extractThemeConfigForCacheKey } from './wasmRenderer';

describe('extractThemeConfigForCacheKey', () => {
  describe('basic theme extraction', () => {
    it('should return default for content without frontmatter', () => {
      const content = '# Hello World\n\nSome content here.';
      expect(extractThemeConfigForCacheKey(content)).toBe('default');
    });

    it('should return default for empty frontmatter', () => {
      const content = '---\ntitle: Test\n---\n\n# Hello';
      expect(extractThemeConfigForCacheKey(content)).toBe('default');
    });

    it('should extract simple theme name', () => {
      const content = '---\ntheme: cosmo\n---\n\n# Hello';
      expect(extractThemeConfigForCacheKey(content)).toBe('cosmo');
    });

    it('should extract theme name with other frontmatter fields', () => {
      const content = '---\ntitle: My Doc\ntheme: darkly\nauthor: Test\n---\n\n# Hello';
      expect(extractThemeConfigForCacheKey(content)).toBe('darkly');
    });
  });

  describe('different themes produce different results', () => {
    it('should return different values for different themes', () => {
      const cosmoDoc = '---\ntheme: cosmo\n---\n\n# Hello';
      const darklyDoc = '---\ntheme: darkly\n---\n\n# Hello';
      const flatlyDoc = '---\ntheme: flatly\n---\n\n# Hello';

      const cosmoConfig = extractThemeConfigForCacheKey(cosmoDoc);
      const darklyConfig = extractThemeConfigForCacheKey(darklyDoc);
      const flatlyConfig = extractThemeConfigForCacheKey(flatlyDoc);

      expect(cosmoConfig).toBe('cosmo');
      expect(darklyConfig).toBe('darkly');
      expect(flatlyConfig).toBe('flatly');

      // All should be different
      expect(cosmoConfig).not.toBe(darklyConfig);
      expect(cosmoConfig).not.toBe(flatlyConfig);
      expect(darklyConfig).not.toBe(flatlyConfig);
    });

    it('should detect when only theme changes in identical documents', () => {
      // This tests the core fix: changing only the theme should produce a different result
      const doc1 = '---\ntitle: Same Title\ntheme: cosmo\n---\n\n# Same Content';
      const doc2 = '---\ntitle: Same Title\ntheme: darkly\n---\n\n# Same Content';

      expect(extractThemeConfigForCacheKey(doc1)).not.toBe(extractThemeConfigForCacheKey(doc2));
    });
  });

  describe('format.html.theme extraction', () => {
    it('should extract theme from format.html.theme structure', () => {
      const content = `---
title: Test
format:
  html:
    theme: journal
---

# Hello`;
      expect(extractThemeConfigForCacheKey(content)).toBe('journal');
    });
  });

  describe('array themes', () => {
    it('should handle inline array theme', () => {
      const content = '---\ntheme: [cosmo, custom.scss]\n---\n\n# Hello';
      expect(extractThemeConfigForCacheKey(content)).toBe('[cosmo, custom.scss]');
    });

    it('should handle multi-line array theme', () => {
      const content = `---
theme:
  - cosmo
  - custom.scss
---

# Hello`;
      // The function extracts the raw value after "theme:", so this captures the array format
      const result = extractThemeConfigForCacheKey(content);
      expect(result).not.toBe('default');
      // The exact format depends on the regex, but it should capture something meaningful
    });
  });

  describe('edge cases', () => {
    it('should handle unclosed frontmatter', () => {
      const content = '---\ntheme: cosmo\n# No closing ---';
      expect(extractThemeConfigForCacheKey(content)).toBe('default');
    });

    it('should handle whitespace before frontmatter', () => {
      const content = '  ---\ntheme: cosmo\n---\n\n# Hello';
      // trimStart() is called, so this should work
      expect(extractThemeConfigForCacheKey(content)).toBe('cosmo');
    });

    it('should handle theme with leading/trailing whitespace', () => {
      const content = '---\ntheme:   sandstone   \n---\n\n# Hello';
      expect(extractThemeConfigForCacheKey(content)).toBe('sandstone');
    });
  });
});

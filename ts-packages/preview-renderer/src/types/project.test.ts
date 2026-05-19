/**
 * Tests for project types and helpers.
 */

import { describe, it, expect } from 'vitest';
import { isQmdFile } from './project';

describe('isQmdFile', () => {
  describe('returns true for QMD files', () => {
    it('handles simple .qmd file', () => {
      expect(isQmdFile('index.qmd')).toBe(true);
    });

    it('handles .qmd file with path', () => {
      expect(isQmdFile('docs/intro.qmd')).toBe(true);
    });

    it('handles uppercase .QMD extension', () => {
      expect(isQmdFile('README.QMD')).toBe(true);
    });

    it('handles mixed case .Qmd extension', () => {
      expect(isQmdFile('document.Qmd')).toBe(true);
    });
  });

  describe('returns false for non-QMD files', () => {
    it('handles .md files', () => {
      expect(isQmdFile('README.md')).toBe(false);
    });

    it('handles .css files', () => {
      expect(isQmdFile('styles.css')).toBe(false);
    });

    it('handles .json files', () => {
      expect(isQmdFile('package.json')).toBe(false);
    });

    it('handles .yml files', () => {
      expect(isQmdFile('config.yml')).toBe(false);
    });

    it('handles .yaml files', () => {
      expect(isQmdFile('_quarto.yaml')).toBe(false);
    });

    it('handles .tsx files', () => {
      expect(isQmdFile('Component.tsx')).toBe(false);
    });

    it('handles .ts files', () => {
      expect(isQmdFile('index.ts')).toBe(false);
    });

    it('handles files without extension', () => {
      expect(isQmdFile('Makefile')).toBe(false);
    });

    it('handles files containing qmd in the name but not extension', () => {
      expect(isQmdFile('qmd-config.json')).toBe(false);
      expect(isQmdFile('myqmd.txt')).toBe(false);
    });
  });

  describe('handles edge cases', () => {
    it('returns false for null', () => {
      expect(isQmdFile(null)).toBe(false);
    });

    it('returns false for undefined', () => {
      expect(isQmdFile(undefined)).toBe(false);
    });

    it('returns false for empty string', () => {
      expect(isQmdFile('')).toBe(false);
    });

    it('handles just .qmd', () => {
      expect(isQmdFile('.qmd')).toBe(true);
    });
  });
});

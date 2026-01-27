/**
 * Tests for stripAnsi utility function
 */

import { describe, it, expect } from 'vitest';
import { stripAnsi } from './stripAnsi';

describe('stripAnsi', () => {
  describe('basic color codes', () => {
    it('should remove red color code', () => {
      expect(stripAnsi('\x1b[31mRed text\x1b[0m')).toBe('Red text');
    });

    it('should remove green color code', () => {
      expect(stripAnsi('\x1b[32mGreen text\x1b[0m')).toBe('Green text');
    });

    it('should remove bold code', () => {
      expect(stripAnsi('\x1b[1mBold text\x1b[0m')).toBe('Bold text');
    });

    it('should remove reset code', () => {
      expect(stripAnsi('text\x1b[0m')).toBe('text');
    });
  });

  describe('extended color codes', () => {
    it('should remove 256-color foreground code', () => {
      expect(stripAnsi('\x1b[38;5;246mGray text\x1b[0m')).toBe('Gray text');
    });

    it('should remove 256-color background code', () => {
      expect(stripAnsi('\x1b[48;5;232mDark background\x1b[0m')).toBe('Dark background');
    });

    it('should remove RGB true-color code', () => {
      expect(stripAnsi('\x1b[38;2;255;100;50mRGB text\x1b[0m')).toBe('RGB text');
    });
  });

  describe('combined codes', () => {
    it('should remove multiple codes in sequence', () => {
      expect(stripAnsi('\x1b[1m\x1b[31mBold red\x1b[0m')).toBe('Bold red');
    });

    it('should remove combined attribute codes', () => {
      expect(stripAnsi('\x1b[1;31mBold red combined\x1b[0m')).toBe('Bold red combined');
    });

    it('should handle interleaved codes and text', () => {
      const input = '\x1b[31mRed\x1b[0m normal \x1b[32mGreen\x1b[0m';
      expect(stripAnsi(input)).toBe('Red normal Green');
    });
  });

  describe('edge cases', () => {
    it('should return unchanged text without ANSI codes', () => {
      expect(stripAnsi('Plain text without codes')).toBe('Plain text without codes');
    });

    it('should handle empty string', () => {
      expect(stripAnsi('')).toBe('');
    });

    it('should preserve newlines', () => {
      expect(stripAnsi('\x1b[31mLine 1\x1b[0m\n\x1b[32mLine 2\x1b[0m')).toBe('Line 1\nLine 2');
    });

    it('should preserve tabs', () => {
      expect(stripAnsi('\x1b[31mCol1\x1b[0m\t\x1b[32mCol2\x1b[0m')).toBe('Col1\tCol2');
    });

    it('should handle text that looks like partial ANSI code', () => {
      // Just an escape character without the full sequence
      expect(stripAnsi('text with \x1b but not a full code')).toBe('text with \x1b but not a full code');
    });

    it('should handle multiple resets in a row', () => {
      expect(stripAnsi('\x1b[0m\x1b[0m\x1b[0mtext\x1b[0m\x1b[0m')).toBe('text');
    });
  });

  describe('real-world examples', () => {
    it('should clean Rust compiler-style error', () => {
      const input = '\x1b[1m\x1b[38;5;9merror\x1b[0m\x1b[1m: expected identifier\x1b[0m';
      expect(stripAnsi(input)).toBe('error: expected identifier');
    });

    it('should clean npm-style warning', () => {
      const input = '\x1b[33mWARN\x1b[0m: package deprecated';
      expect(stripAnsi(input)).toBe('WARN: package deprecated');
    });
  });
});

/**
 * Tests for diffToMonacoEdits utility.
 *
 * These tests verify that the diff-based edit computation correctly
 * produces Monaco edit operations that transform source content into
 * target content while preserving cursor-friendly edit semantics.
 */

import { describe, it, expect } from 'vitest';
import { diffToMonacoEdits } from './diffToMonacoEdits';

describe('diffToMonacoEdits', () => {
  describe('basic operations', () => {
    it('returns empty array when content is identical', () => {
      const edits = diffToMonacoEdits('hello', 'hello');
      expect(edits).toEqual([]);
    });

    it('returns empty array for empty strings', () => {
      const edits = diffToMonacoEdits('', '');
      expect(edits).toEqual([]);
    });

    it('handles simple insertion at the end', () => {
      const edits = diffToMonacoEdits('hello', 'hello world');
      expect(edits).toHaveLength(1);
      expect(edits[0]).toMatchObject({
        range: {
          startLineNumber: 1,
          startColumn: 6,
          endLineNumber: 1,
          endColumn: 6,
        },
        text: ' world',
        forceMoveMarkers: true,
      });
    });

    it('handles simple insertion at the beginning', () => {
      const edits = diffToMonacoEdits('world', 'hello world');
      expect(edits).toHaveLength(1);
      expect(edits[0]).toMatchObject({
        range: {
          startLineNumber: 1,
          startColumn: 1,
          endLineNumber: 1,
          endColumn: 1,
        },
        text: 'hello ',
        forceMoveMarkers: true,
      });
    });

    it('handles simple deletion at the end', () => {
      const edits = diffToMonacoEdits('hello world', 'hello');
      expect(edits).toHaveLength(1);
      expect(edits[0]).toMatchObject({
        range: {
          startLineNumber: 1,
          startColumn: 6,
          endLineNumber: 1,
          endColumn: 12,
        },
        text: '',
        forceMoveMarkers: false,
      });
    });

    it('handles simple deletion at the beginning', () => {
      const edits = diffToMonacoEdits('hello world', 'world');
      expect(edits).toHaveLength(1);
      expect(edits[0]).toMatchObject({
        range: {
          startLineNumber: 1,
          startColumn: 1,
          endLineNumber: 1,
          endColumn: 7,
        },
        text: '',
        forceMoveMarkers: false,
      });
    });

    it('handles simple replacement', () => {
      const edits = diffToMonacoEdits('hello', 'hallo');
      // Should produce delete 'e' then insert 'a', or a single replacement
      expect(edits.length).toBeGreaterThan(0);

      // Verify applying edits would produce correct result
      // by checking the text operations
      const hasDelete = edits.some(e => e.text === '' && e.range.startColumn !== e.range.endColumn);
      const hasInsert = edits.some(e => e.text === 'a');
      expect(hasDelete || hasInsert).toBe(true);
    });
  });

  describe('multiline operations', () => {
    it('handles insertion of newline', () => {
      const edits = diffToMonacoEdits('hello', 'hel\nlo');
      expect(edits).toHaveLength(1);
      expect(edits[0]).toMatchObject({
        range: {
          startLineNumber: 1,
          startColumn: 4,
          endLineNumber: 1,
          endColumn: 4,
        },
        text: '\n',
        forceMoveMarkers: true,
      });
    });

    it('handles deletion across lines', () => {
      const edits = diffToMonacoEdits('hello\nworld', 'helloworld');
      expect(edits).toHaveLength(1);
      expect(edits[0]).toMatchObject({
        range: {
          startLineNumber: 1,
          startColumn: 6,
          endLineNumber: 2,
          endColumn: 1,
        },
        text: '',
        forceMoveMarkers: false,
      });
    });

    it('handles insertion of multiple lines', () => {
      const edits = diffToMonacoEdits('start\nend', 'start\nmiddle1\nmiddle2\nend');
      expect(edits).toHaveLength(1);
      expect(edits[0]).toMatchObject({
        text: 'middle1\nmiddle2\n',
        forceMoveMarkers: true,
      });
    });

    it('correctly calculates positions after newlines', () => {
      const edits = diffToMonacoEdits('line1\nline2\nline3', 'line1\nline2\nline3X');
      expect(edits).toHaveLength(1);
      expect(edits[0]).toMatchObject({
        range: {
          startLineNumber: 3,
          startColumn: 6,
          endLineNumber: 3,
          endColumn: 6,
        },
        text: 'X',
        forceMoveMarkers: true,
      });
    });
  });

  describe('complex operations', () => {
    it('handles multiple separate changes', () => {
      const edits = diffToMonacoEdits('abc def ghi', 'ABC def GHI');
      // Should have edits for 'abc' -> 'ABC' and 'ghi' -> 'GHI'
      expect(edits.length).toBeGreaterThanOrEqual(2);
    });

    it('handles complete content replacement', () => {
      const edits = diffToMonacoEdits('completely different', 'totally new content');
      expect(edits.length).toBeGreaterThan(0);
    });

    it('handles empty to non-empty', () => {
      const edits = diffToMonacoEdits('', 'new content');
      expect(edits).toHaveLength(1);
      expect(edits[0]).toMatchObject({
        range: {
          startLineNumber: 1,
          startColumn: 1,
          endLineNumber: 1,
          endColumn: 1,
        },
        text: 'new content',
        forceMoveMarkers: true,
      });
    });

    it('handles non-empty to empty', () => {
      const edits = diffToMonacoEdits('old content', '');
      expect(edits).toHaveLength(1);
      expect(edits[0]).toMatchObject({
        text: '',
        forceMoveMarkers: false,
      });
    });
  });

  describe('cursor-preserving semantics', () => {
    it('uses forceMoveMarkers: true for insertions', () => {
      const edits = diffToMonacoEdits('ab', 'aXb');
      const insertions = edits.filter(e => e.text !== '');
      insertions.forEach(edit => {
        expect(edit.forceMoveMarkers).toBe(true);
      });
    });

    it('uses forceMoveMarkers: false for deletions', () => {
      const edits = diffToMonacoEdits('aXb', 'ab');
      const deletions = edits.filter(e => e.text === '');
      deletions.forEach(edit => {
        expect(edit.forceMoveMarkers).toBe(false);
      });
    });
  });

  describe('edge cases', () => {
    it('handles unicode characters', () => {
      const edits = diffToMonacoEdits('hello 世界', 'hello 世界!');
      expect(edits).toHaveLength(1);
      expect(edits[0].text).toBe('!');
    });

    it('handles emoji', () => {
      const edits = diffToMonacoEdits('hello 👋', 'hello 👋🌍');
      expect(edits).toHaveLength(1);
      // Note: emoji are multi-byte but should be handled correctly
    });

    it('handles tabs', () => {
      const edits = diffToMonacoEdits('a\tb', 'a\t\tb');
      expect(edits).toHaveLength(1);
      expect(edits[0].text).toBe('\t');
    });

    it('handles carriage returns', () => {
      const edits = diffToMonacoEdits('line1\r\nline2', 'line1\nline2');
      expect(edits.length).toBeGreaterThan(0);
    });
  });

  describe('realistic collaborative editing scenarios', () => {
    it('handles concurrent insertion at different positions', () => {
      // User A has "hello world", User B inserted "X" after "hello"
      // Simulating what would happen if we need to sync
      const localContent = 'hello world';
      const remoteContent = 'helloX world';

      const edits = diffToMonacoEdits(localContent, remoteContent);
      expect(edits).toHaveLength(1);
      expect(edits[0]).toMatchObject({
        range: {
          startLineNumber: 1,
          startColumn: 6,
          endLineNumber: 1,
          endColumn: 6,
        },
        text: 'X',
        forceMoveMarkers: true,
      });
    });

    it('handles typing at the end while remote changes arrive', () => {
      // Local: "hello wor" (user typing "world")
      // Remote merged content has: "Xhello world" (remote added "X" at start and "ld" at end)
      const localContent = 'hello wor';
      const remoteContent = 'Xhello world';

      const edits = diffToMonacoEdits(localContent, remoteContent);
      // Should have insertion at start and insertion at end
      expect(edits.length).toBe(2);
    });

    it('handles rapid typing scenario - single character additions', () => {
      // Simulating rapid typing where content diverges slightly
      const localContent = 'The quick brown';
      const remoteContent = 'The quick brown fox';

      const edits = diffToMonacoEdits(localContent, remoteContent);
      expect(edits).toHaveLength(1);
      expect(edits[0].text).toBe(' fox');
    });
  });
});

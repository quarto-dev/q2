/**
 * Convert a text diff to Monaco editor edit operations.
 *
 * This utility enables synchronization between external content (like Automerge)
 * and Monaco by computing the minimal edits needed to transform Monaco's content
 * to match the target content, preserving cursor position.
 */

import diff from 'fast-diff';
import type * as Monaco from 'monaco-editor';

// fast-diff operation constants
const DIFF_DELETE = -1;
const DIFF_EQUAL = 0;
const DIFF_INSERT = 1;

/**
 * Convert a character offset to Monaco position (1-indexed line/column).
 *
 * @param content - The document content
 * @param offset - Character offset (0-indexed)
 * @returns Monaco position with 1-indexed lineNumber and column
 */
function offsetToPosition(content: string, offset: number): Monaco.IPosition {
  // Clamp offset to valid range
  const clampedOffset = Math.max(0, Math.min(offset, content.length));

  let line = 1;
  let column = 1;

  for (let i = 0; i < clampedOffset; i++) {
    if (content[i] === '\n') {
      line++;
      column = 1;
    } else {
      column++;
    }
  }

  return { lineNumber: line, column };
}

/**
 * Compute Monaco edit operations to transform `currentContent` into `targetContent`.
 *
 * Uses the fast-diff library to compute a minimal diff, then converts
 * the diff operations into Monaco IIdentifiedSingleEditOperation objects
 * that can be applied via editor.executeEdits().
 *
 * @param currentContent - The current content in Monaco
 * @param targetContent - The desired content (e.g., from Automerge)
 * @returns Array of Monaco edit operations
 */
export function diffToMonacoEdits(
  currentContent: string,
  targetContent: string
): Monaco.editor.IIdentifiedSingleEditOperation[] {
  // Fast path: if content is identical, no edits needed
  if (currentContent === targetContent) {
    return [];
  }

  const diffs = diff(currentContent, targetContent);
  const edits: Monaco.editor.IIdentifiedSingleEditOperation[] = [];

  // Track position in the original (current) content
  let currentOffset = 0;

  for (const [operation, text] of diffs) {
    if (operation === DIFF_EQUAL) {
      // Equal: advance position, no edit needed
      currentOffset += text.length;
    } else if (operation === DIFF_DELETE) {
      // Delete: remove text from currentOffset
      const startPos = offsetToPosition(currentContent, currentOffset);
      const endPos = offsetToPosition(currentContent, currentOffset + text.length);

      edits.push({
        range: {
          startLineNumber: startPos.lineNumber,
          startColumn: startPos.column,
          endLineNumber: endPos.lineNumber,
          endColumn: endPos.column,
        },
        text: '',
        forceMoveMarkers: false,
      });

      currentOffset += text.length;
    } else if (operation === DIFF_INSERT) {
      // Insert: add text at currentOffset
      const pos = offsetToPosition(currentContent, currentOffset);

      edits.push({
        range: {
          startLineNumber: pos.lineNumber,
          startColumn: pos.column,
          endLineNumber: pos.lineNumber,
          endColumn: pos.column,
        },
        text: text,
        forceMoveMarkers: true,
      });

      // Insert doesn't advance position in original content
    }
  }

  return edits;
}

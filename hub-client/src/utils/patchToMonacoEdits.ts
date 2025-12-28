/**
 * Convert Automerge patches to Monaco editor edit operations.
 *
 * This utility enables incremental document updates in Monaco, preserving
 * cursor position during collaborative editing by applying targeted edits
 * instead of replacing the entire document content.
 */

import type { Patch, SpliceTextPatch, DelPatch } from '@automerge/automerge';
import type * as Monaco from 'monaco-editor';

/**
 * Convert a character offset to Monaco position (1-indexed line/column).
 *
 * @param content - The document content before the edit
 * @param offset - Character offset (0-indexed)
 * @returns Monaco position with 1-indexed lineNumber and column
 */
export function offsetToPosition(content: string, offset: number): Monaco.IPosition {
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
 * Type guard for SpliceTextPatch (text insertion).
 */
function isSpliceTextPatch(patch: Patch): patch is SpliceTextPatch {
  return patch.action === 'splice';
}

/**
 * Type guard for DelPatch (text deletion).
 */
function isDelPatch(patch: Patch): patch is DelPatch {
  return patch.action === 'del';
}

/**
 * Extract the character index from a patch path.
 *
 * Automerge patch paths for text operations are structured as:
 * ["text", index] where index is the character position.
 *
 * @param path - The patch path array
 * @returns The character index, or null if not a text operation
 */
function getTextIndex(path: (string | number)[]): number | null {
  // We expect paths like ["text", index] for text field operations
  if (path.length < 2) return null;
  if (path[0] !== 'text') return null;

  const index = path[1];
  if (typeof index !== 'number') return null;

  return index;
}

/**
 * Convert Automerge patches to Monaco edit operations.
 *
 * Only processes patches that target the 'text' field. Each patch is
 * converted to a Monaco IIdentifiedSingleEditOperation that can be
 * applied via editor.executeEdits().
 *
 * @param patches - Array of Automerge patches from a change event
 * @param currentContent - The document content before these patches
 * @returns Array of Monaco edit operations
 */
export function patchesToMonacoEdits(
  patches: Patch[],
  currentContent: string
): Monaco.editor.IIdentifiedSingleEditOperation[] {
  const edits: Monaco.editor.IIdentifiedSingleEditOperation[] = [];

  // Track working content as we process patches.
  // Each patch modifies the content, affecting positions for subsequent patches.
  let workingContent = currentContent;

  for (const patch of patches) {
    if (isSpliceTextPatch(patch)) {
      // Text insertion
      const index = getTextIndex(patch.path);
      if (index === null) continue;

      const adjustedIndex = index;
      const pos = offsetToPosition(workingContent, adjustedIndex);

      edits.push({
        range: {
          startLineNumber: pos.lineNumber,
          startColumn: pos.column,
          endLineNumber: pos.lineNumber,
          endColumn: pos.column,
        },
        text: patch.value,
        forceMoveMarkers: true,
      });

      // Update working content for subsequent patch position calculations
      workingContent =
        workingContent.slice(0, adjustedIndex) +
        patch.value +
        workingContent.slice(adjustedIndex);
    } else if (isDelPatch(patch)) {
      // Text deletion
      const index = getTextIndex(patch.path);
      if (index === null) continue;

      // Delete count defaults to 1 if not specified
      const deleteCount = patch.length ?? 1;
      const adjustedIndex = index;

      const startPos = offsetToPosition(workingContent, adjustedIndex);
      const endPos = offsetToPosition(workingContent, adjustedIndex + deleteCount);

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

      // Update working content for subsequent patch position calculations
      workingContent =
        workingContent.slice(0, adjustedIndex) +
        workingContent.slice(adjustedIndex + deleteCount);
    }
    // Other patch types (put, mark, insert, etc.) are ignored
    // as they don't apply to plain text editing
  }

  return edits;
}

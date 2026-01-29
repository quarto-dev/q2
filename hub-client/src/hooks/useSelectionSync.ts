/**
 * React hook for bidirectional selection synchronization between Monaco editor and preview iframe.
 *
 * Features:
 * - Preview → Editor: Selection in preview sets corresponding selection in editor
 * - Editor → Preview: Selection in editor sets corresponding selection in preview
 * - Uses data-loc attributes from anchor and focus nodes to determine range
 * - Prevents feedback loops using directional sync guards
 */

import { useCallback, useEffect, useRef } from 'react';
import type { RefObject } from 'react';
import type * as Monaco from 'monaco-editor';
import type { SourceLocation } from '../components/DoubleBufferedIframe';
import type { MorphIframeHandle } from '../components/MorphIframe';

interface UseSelectionSyncOptions {
  /** Reference to Monaco editor instance */
  editorRef: RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  /** Reference to MorphIframe instance */
  previewRef: RefObject<MorphIframeHandle | null>;
  /** Whether selection sync is enabled */
  enabled: boolean;
}

/**
 * Return type: callback for preview to call when selection changes
 */
interface UseSelectionSyncReturn {
  handlePreviewSelection: (startPos: SourceLocation | null, endPos: SourceLocation | null) => void;
}

/**
 * Hook for bidirectional selection synchronization between Monaco editor and preview iframe.
 * Returns callback that should be passed to the preview component.
 */
export function useSelectionSync({
  editorRef,
  previewRef,
  enabled,
}: UseSelectionSyncOptions): UseSelectionSyncReturn {
  // Track if we're currently syncing to prevent feedback loops
  const isSyncingRef = useRef(false);

  // Preview → Editor selection sync
  const handlePreviewSelection = useCallback((
    startPos: SourceLocation | null,
    endPos: SourceLocation | null
  ) => {
    const editor = editorRef.current;
    if (!editor || !startPos || !endPos) return;

    // Prevent feedback loop: if we're already syncing, don't sync again
    if (isSyncingRef.current) return;

    isSyncingRef.current = true;

    // Create Monaco range from source locations
    // data-loc uses 1-based line/column numbers, which Monaco also uses
    const range = {
      startLineNumber: startPos.startLine,
      startColumn: startPos.startCol,
      endLineNumber: endPos.endLine,
      endColumn: endPos.endCol,
    };

    // Set the selection in the editor
    editor.setSelection(range);

    // Reveal the selection in the editor viewport
    editor.revealRangeInCenter(range);

    // Optionally focus the editor
    editor.focus();

    // Reset sync flag after a short delay
    setTimeout(() => {
      isSyncingRef.current = false;
    }, 50);
  }, [editorRef]);

  // Editor → Preview selection sync
  useEffect(() => {
    if (!enabled) return;

    const editor = editorRef.current;
    const preview = previewRef.current;
    if (!editor || !preview) return;

    // Clear preview selection when editor content changes
    const contentDisposable = editor.onDidChangeModelContent(() => {
      // Clear the preview selection to prevent stale selections from syncing back
      preview.clearSelection();
    });

    // Listen for selection changes in the editor
    const selectionDisposable = editor.onDidChangeCursorSelection((e) => {
      const selection = e.selection;

      // Only sync if there's an actual selection (not just a cursor)
      // A collapsed selection means start === end (just a cursor, no selection)
      const isCollapsed = selection.startLineNumber === selection.endLineNumber &&
        selection.startColumn === selection.endColumn;

      if (isCollapsed) return;

      // Only sync if the selection change was user-initiated (mouse or keyboard navigation)
      // Don't sync if it was from typing, deleting, pasting, etc.
      // source can be: 'keyboard', 'mouse', 'api', 'modelChange'
      if (e.source !== 'mouse' && e.source !== 'keyboard') return;

      // For keyboard source, only sync if it's not from editing
      // Check the reason - we want explicit selection changes, not edits
      if (e.source === 'keyboard' && e.reason !== 3) return; // 3 = CursorChangeReason.Explicit

      // Prevent feedback loop: if we're already syncing, don't sync again
      if (isSyncingRef.current) return;

      isSyncingRef.current = true;

      // Convert Monaco selection to SourceLocation format
      // Assuming fileId 0 for the current file (we don't track multiple files in selection)
      const startPos: SourceLocation = {
        fileId: 0,
        startLine: selection.startLineNumber,
        startCol: selection.startColumn,
        endLine: selection.startLineNumber,
        endCol: selection.startColumn,
      };

      const endPos: SourceLocation = {
        fileId: 0,
        startLine: selection.endLineNumber,
        startCol: selection.endColumn,
        endLine: selection.endLineNumber,
        endCol: selection.endColumn,
      };
      // Set the selection in the preview
      preview.setSelection(startPos, endPos);

      // Reset sync flag after a short delay
      setTimeout(() => {
        isSyncingRef.current = false;
      }, 50);
    });

    return () => {
      contentDisposable.dispose();
      selectionDisposable.dispose();
    };
  }, [enabled, editorRef, previewRef]);

  return {
    handlePreviewSelection,
  };
}

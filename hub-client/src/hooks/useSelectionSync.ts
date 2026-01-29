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

    // Listen for selection changes in the editor
    const disposable = editor.onDidChangeCursorSelection((e) => {
      // Prevent feedback loop: if we're already syncing, don't sync again
      if (isSyncingRef.current) return;

      isSyncingRef.current = true;

      const selection = e.selection;

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
      disposable.dispose();
    };
  }, [enabled, editorRef, previewRef]);

  return {
    handlePreviewSelection,
  };
}

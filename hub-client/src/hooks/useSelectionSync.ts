/**
 * React hook for selection synchronization from preview iframe to Monaco editor.
 *
 * Features:
 * - Preview → Editor: Selection in preview sets corresponding selection in editor
 * - Uses data-loc attributes from anchor and focus nodes to determine range
 */

import { useCallback } from 'react';
import type { RefObject } from 'react';
import type * as Monaco from 'monaco-editor';
import type { SourceLocation } from '../components/DoubleBufferedIframe';

interface UseSelectionSyncOptions {
  /** Reference to Monaco editor instance */
  editorRef: RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
}

/**
 * Return type: callback for preview to call when selection changes
 */
interface UseSelectionSyncReturn {
  handlePreviewSelection: (startPos: SourceLocation | null, endPos: SourceLocation | null) => void;
}

/**
 * Hook for selection synchronization from preview iframe to Monaco editor.
 * Returns callback that should be passed to the preview component.
 */
export function useSelectionSync({
  editorRef,
}: UseSelectionSyncOptions): UseSelectionSyncReturn {
  // Preview → Editor selection sync
  const handlePreviewSelection = useCallback((
    startPos: SourceLocation | null,
    endPos: SourceLocation | null
  ) => {
    const editor = editorRef.current;
    if (!editor || !startPos || !endPos) return;

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
  }, [editorRef]);

  return {
    handlePreviewSelection,
  };
}

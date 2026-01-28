/**
 * React hook for bidirectional scroll synchronization between Monaco editor and preview iframe.
 *
 * Features:
 * - Editor → Preview: Cursor movement scrolls preview to corresponding content
 * - Preview → Editor: Scroll in preview scrolls editor viewport (without moving cursor)
 * - 50ms debounce to prevent jitter
 * - Graceful degradation when source locations unavailable
 */

import { useEffect, useRef, useCallback } from 'react';
import type { RefObject } from 'react';
import type * as Monaco from 'monaco-editor';

interface UseScrollSyncOptions {
  /** Reference to Monaco editor instance */
  editorRef: RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  /** Function to scroll preview to a specific line (provided by DoubleBufferedIframe) */
  scrollPreviewToLine: (line: number) => void;
  /** Function to get the preview's scroll ratio (provided by DoubleBufferedIframe) */
  getPreviewScrollRatio: () => number | null;
  /** Whether scroll sync is enabled */
  enabled: boolean;
  /** Reference tracking whether editor has focus (to prevent feedback loop) */
  editorHasFocusRef: RefObject<boolean>;
}

/**
 * Return type: callbacks for preview to call when scrolled or clicked
 */
interface UseScrollSyncReturn {
  handlePreviewScroll: () => void;
  handlePreviewClick: () => void;
}

/**
 * Hook for bidirectional scroll synchronization between Monaco editor and preview iframe.
 * Returns callbacks that should be passed to the preview component.
 */
export function useScrollSync({
  editorRef,
  scrollPreviewToLine,
  getPreviewScrollRatio,
  enabled,
  editorHasFocusRef,
}: UseScrollSyncOptions): UseScrollSyncReturn {
  // Track if we're currently syncing to prevent feedback loops
  const isSyncingRef = useRef(false);

  // Debounce timers
  const editorDebounceRef = useRef<number | null>(null);
  const previewDebounceRef = useRef<number | null>(null);

  // Editor → Preview sync
  const syncEditorToPreview = useCallback(() => {
    if (!enabled || isSyncingRef.current) return;

    const editor = editorRef.current;
    if (!editor) return;

    const position = editor.getPosition();
    if (!position) return;

    const line = position.lineNumber;

    isSyncingRef.current = true;
    scrollPreviewToLine(line);
    // Reset syncing flag after animation completes
    setTimeout(() => {
      isSyncingRef.current = false;
    }, 300);
  }, [enabled, editorRef, scrollPreviewToLine]);

  // Preview → Editor sync (using scroll ratio matching)
  const syncPreviewToEditor = useCallback(() => {
    // Skip if disabled, already syncing, or editor has focus (prevents feedback loop)
    if (!enabled || isSyncingRef.current || editorHasFocusRef.current) return;

    const editor = editorRef.current;
    if (!editor) return;

    const scrollRatio = getPreviewScrollRatio();
    if (scrollRatio === null) return;

    // Apply same ratio to editor
    const editorScrollHeight = editor.getScrollHeight();
    const editorViewportHeight = editor.getLayoutInfo().height;
    const editorMaxScroll = editorScrollHeight - editorViewportHeight;

    const editorScrollTop = scrollRatio * editorMaxScroll;

    isSyncingRef.current = true;
    // Use smooth scrolling (ScrollType.Smooth = 1)
    editor.setScrollTop(editorScrollTop, 1);
    setTimeout(() => {
      isSyncingRef.current = false;
    }, 300); // Longer timeout to account for smooth animation
  }, [enabled, editorRef, getPreviewScrollRatio, editorHasFocusRef]);

  // Set up editor cursor position listener
  useEffect(() => {
    if (!enabled) return;

    const editor = editorRef.current;
    if (!editor) return;

    const disposable = editor.onDidChangeCursorPosition(() => {
      // Debounce
      if (editorDebounceRef.current) {
        clearTimeout(editorDebounceRef.current);
      }
      editorDebounceRef.current = window.setTimeout(() => {
        syncEditorToPreview();
      }, 50);
    });

    return () => {
      disposable.dispose();
      if (editorDebounceRef.current) {
        clearTimeout(editorDebounceRef.current);
      }
    };
  }, [enabled, editorRef, syncEditorToPreview]);

  // Debounced preview scroll handler
  const handlePreviewScroll = useCallback(() => {
    // Debounce
    if (previewDebounceRef.current) {
      clearTimeout(previewDebounceRef.current);
    }
    previewDebounceRef.current = window.setTimeout(() => {
      syncPreviewToEditor();
    }, 50);
  }, [syncPreviewToEditor]);

  // Preview click handler (sync immediately, no debounce)
  const handlePreviewClick = useCallback(() => {
    syncPreviewToEditor();
  }, [syncPreviewToEditor]);

  // Cleanup debounce timer on unmount
  useEffect(() => {
    return () => {
      if (previewDebounceRef.current) {
        clearTimeout(previewDebounceRef.current);
      }
    };
  }, []);

  return {
    handlePreviewScroll,
    handlePreviewClick,
  };
}

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

/**
 * Parsed source location from data-loc attribute.
 * Format: "fileId:startLine:startCol-endLine:endCol" (1-based)
 */
interface SourceLocation {
  fileId: number;
  startLine: number;
  startCol: number;
  endLine: number;
  endCol: number;
}

/**
 * Parse a data-loc attribute string into a SourceLocation object.
 * Returns null if the format is invalid.
 */
function parseDataLoc(dataLoc: string): SourceLocation | null {
  const match = dataLoc.match(/^(\d+):(\d+):(\d+)-(\d+):(\d+)$/);
  if (!match) return null;
  return {
    fileId: parseInt(match[1], 10),
    startLine: parseInt(match[2], 10),
    startCol: parseInt(match[3], 10),
    endLine: parseInt(match[4], 10),
    endCol: parseInt(match[5], 10),
  };
}

/**
 * Find the best matching element for a given line number.
 * Prefers the most specific (smallest range) match.
 */
function findElementForLine(
  doc: Document,
  line: number
): HTMLElement | null {
  const elements = doc.querySelectorAll('[data-loc]');
  let bestMatch: HTMLElement | null = null;
  let bestRangeSize = Infinity;

  for (const element of elements) {
    const dataLoc = element.getAttribute('data-loc');
    if (!dataLoc) continue;

    const loc = parseDataLoc(dataLoc);
    if (!loc) continue;

    // Check if line is within this element's range
    if (line >= loc.startLine && line <= loc.endLine) {
      const rangeSize = loc.endLine - loc.startLine;
      // Prefer smaller (more specific) ranges
      if (rangeSize < bestRangeSize) {
        bestMatch = element as HTMLElement;
        bestRangeSize = rangeSize;
      }
    }
  }

  return bestMatch;
}

/**
 * Check if an element is fully visible in the viewport.
 */
function isElementVisible(element: HTMLElement): boolean {
  const rect = element.getBoundingClientRect();
  const viewportHeight = window.innerHeight;

  // Element is visible if it's within the viewport bounds
  return rect.top >= 0 && rect.bottom <= viewportHeight;
}

interface UseScrollSyncOptions {
  /** Reference to Monaco editor instance */
  editorRef: RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  /** Reference to preview iframe element */
  iframeRef: RefObject<HTMLIFrameElement | null>;
  /** Whether scroll sync is enabled */
  enabled: boolean;
  /** Called when iframe content is loaded/updated (increments to trigger re-indexing) */
  iframeLoadCount: number;
  /** Reference tracking whether editor has focus (to prevent feedback loop) */
  editorHasFocusRef: RefObject<boolean>;
}

/**
 * Hook for bidirectional scroll synchronization between Monaco editor and preview iframe.
 */
export function useScrollSync({
  editorRef,
  iframeRef,
  enabled,
  iframeLoadCount,
  editorHasFocusRef,
}: UseScrollSyncOptions): void {
  // Track if we're currently syncing to prevent feedback loops
  const isSyncingRef = useRef(false);

  // Debounce timers
  const editorDebounceRef = useRef<number | null>(null);
  const previewDebounceRef = useRef<number | null>(null);

  // Editor → Preview sync
  const syncEditorToPreview = useCallback(() => {
    if (!enabled || isSyncingRef.current) return;

    const editor = editorRef.current;
    const iframe = iframeRef.current;
    if (!editor || !iframe?.contentDocument) return;

    const position = editor.getPosition();
    if (!position) return;

    const line = position.lineNumber;
    const doc = iframe.contentDocument;

    const element = findElementForLine(doc, line);
    if (!element) return;

    // Only scroll if element is not already visible
    if (!isElementVisible(element)) {
      isSyncingRef.current = true;
      element.scrollIntoView({ behavior: 'smooth', block: 'center' });
      // Reset syncing flag after animation completes
      setTimeout(() => {
        isSyncingRef.current = false;
      }, 300);
    }
  }, [enabled, editorRef, iframeRef]);

  // Preview → Editor sync (using scroll ratio matching)
  const syncPreviewToEditor = useCallback(() => {
    // Skip if disabled, already syncing, or editor has focus (prevents feedback loop)
    if (!enabled || isSyncingRef.current || editorHasFocusRef.current) return;

    const editor = editorRef.current;
    const iframe = iframeRef.current;
    if (!editor || !iframe?.contentWindow || !iframe?.contentDocument) return;

    const iframeWindow = iframe.contentWindow;
    const iframeDoc = iframe.contentDocument;

    // Calculate preview scroll ratio (0 = top, 1 = bottom)
    const previewScrollY = iframeWindow.scrollY;
    const previewScrollHeight = iframeDoc.documentElement.scrollHeight;
    const previewViewportHeight = iframeWindow.innerHeight;
    const previewMaxScroll = previewScrollHeight - previewViewportHeight;

    // Avoid division by zero for short documents
    if (previewMaxScroll <= 0) return;

    const scrollRatio = previewScrollY / previewMaxScroll;

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
  }, [enabled, editorRef, iframeRef, editorHasFocusRef]);

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

  // Set up preview scroll and click listeners
  useEffect(() => {
    if (!enabled) return;

    const iframe = iframeRef.current;
    if (!iframe?.contentWindow || !iframe?.contentDocument) return;

    const handleScroll = () => {
      // Debounce
      if (previewDebounceRef.current) {
        clearTimeout(previewDebounceRef.current);
      }
      previewDebounceRef.current = window.setTimeout(() => {
        syncPreviewToEditor();
      }, 50);
    };

    const handleClick = () => {
      // Sync immediately on click (no debounce needed)
      syncPreviewToEditor();
    };

    // Listen to scroll on the iframe's content window
    iframe.contentWindow.addEventListener('scroll', handleScroll, { passive: true });
    // Listen to click on the iframe's document to sync when user clicks on preview
    iframe.contentDocument.addEventListener('click', handleClick);

    return () => {
      iframe.contentWindow?.removeEventListener('scroll', handleScroll);
      iframe.contentDocument?.removeEventListener('click', handleClick);
      if (previewDebounceRef.current) {
        clearTimeout(previewDebounceRef.current);
      }
    };
  }, [enabled, iframeRef, iframeLoadCount, syncPreviewToEditor]);
}

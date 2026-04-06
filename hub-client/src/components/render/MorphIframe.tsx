import { useRef, useEffect, useCallback, useImperativeHandle } from 'react';
import type { Ref } from 'react';
import morphdom from 'morphdom';
import { postProcessIframe } from '../../utils/iframePostProcessor';

// Methods exposed via ref
export interface MorphIframeHandle {
  scrollToLine: (line: number) => void;
  getScrollRatio: () => number | null;
  setScrollRatio: (ratio: number) => void;
  setSelection: (startPos: SourceLocation, endPos: SourceLocation) => void;
  clearSelection: () => void;
}

interface MorphIframeProps {
  // HTML content to render - component handles morphing automatically
  html: string;
  // Current file path for resolving relative links
  currentFilePath: string;

  qmdContent: string;
  // Callback when user navigates to a different document (with optional anchor)
  // Parent (Preview) handles file lookup and switching
  onNavigateToDocument: (targetPath: string, anchor: string | null) => void;
  // Optional callback when preview is scrolled
  onScroll?: () => void;
  // Optional callback when preview is clicked
  onClick?: () => void;
  // Optional callback when selection changes in preview
  onSelectionChange?: (startPos: SourceLocation | null, endPos: SourceLocation | null) => void;
  // Ref to expose imperative methods
  ref: Ref<MorphIframeHandle>;
}

/**
 * Parsed source location from data-loc attribute.
 * Format: "fileId:startLine:startCol-endLine:endCol" (1-based)
 */
export interface SourceLocation {
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

/**
 * Check if a position (line, col) is within or after the start of a data-loc range.
 */
function isPositionAfterOrAt(
  targetLine: number,
  targetCol: number,
  startLine: number,
  startCol: number
): boolean {
  if (targetLine > startLine) return true;
  if (targetLine === startLine && targetCol >= startCol) return true;
  return false;
}

/**
 * Check if a position (line, col) is within or before the end of a data-loc range.
 */
function isPositionBeforeOrAt(
  targetLine: number,
  targetCol: number,
  endLine: number,
  endCol: number
): boolean {
  if (targetLine < endLine) return true;
  if (targetLine === endLine && targetCol <= endCol) return true;
  return false;
}

/**
 * Convert (row, col) position to character offset from start of text.
 *
 * @param text - The source text
 * @param row - 1-based row number
 * @param col - 1-based column number
 * @returns Character offset from start of text, or null if position is out of bounds
 */
function rowAndColToOffset(
  text: string,
  row: number,
  col: number
): number | null {
  const lines = text.split('\n');

  // Validate input position
  if (row < 1 || row > lines.length) return null;
  if (col < 1 || col > lines[row - 1].length + 1) return null;

  // Calculate character offset from start of text
  let charOffset = 0;
  for (let i = 0; i < row - 1; i++) {
    charOffset += lines[i].length + 1; // +1 for newline
  }
  charOffset += col - 1;

  return charOffset;
}

/**
 * Convert character offset to (row, col) position.
 *
 * @param text - The source text
 * @param offset - Character offset from start of text
 * @returns (row, col) position (1-based), or null if offset is out of bounds
 */
function offsetToRowAndCol(
  text: string,
  offset: number
): { row: number, col: number } | null {
  // Validate offset is within bounds
  if (offset < 0 || offset > text.length) return null;

  const lines = text.split('\n');
  let currentOffset = 0;

  for (let i = 0; i < lines.length; i++) {
    const lineLength = lines[i].length;
    const lineEnd = currentOffset + lineLength;

    if (offset <= lineEnd) {
      return {
        row: i + 1,
        col: offset - currentOffset + 1
      };
    }

    currentOffset = lineEnd + 1; // +1 for newline
  }

  // Should not reach here if bounds check passed
  return null;
}

/**
 * Add a character offset to a position (row, col) in a string.
 *
 * @param text - The source text
 * @param row - 1-based row number
 * @param col - 1-based column number
 * @param offset - Number of characters to add (can be negative)
 * @returns New (row, col) position after applying offset, or null if out of bounds
 */
function addOffsetToPosition(
  text: string,
  row: number,
  col: number,
  offset: number
): { row: number, col: number } | null {
  const charOffset = rowAndColToOffset(text, row, col);
  if (charOffset === null) return null;

  return offsetToRowAndCol(text, charOffset + offset);
}

/**
 * Morph-based iframe component for seamless updates.
 *
 * Uses morphdom to update the iframe's content in-place, preserving:
 * - Scroll position
 * - DOM state (expanded/collapsed elements, etc.)
 * - Better performance for small changes
 *
 * When new HTML arrives:
 * 1. Saves current scroll position
 * 2. Uses morphdom to morph the iframe's document into the new HTML
 * 3. Post-processes the updated content (CSS, link handlers, etc.)
 * 4. Restores scroll position
 */
function MorphIframe({
  html,
  currentFilePath,
  qmdContent,
  onNavigateToDocument,
  onScroll,
  onClick,
  onSelectionChange,
  ref,
}: MorphIframeProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const isInitializedRef = useRef(false);

  // Scroll the preview to an anchor element
  const scrollToAnchor = useCallback((anchor: string) => {
    const iframe = iframeRef.current;
    const doc = iframe?.contentDocument;
    if (!doc) return;

    const element = doc.getElementById(anchor);
    if (element) {
      element.scrollIntoView({ behavior: 'instant', block: 'start' });
    }
  }, []);

  // Handler for .qmd link clicks and anchor clicks in the preview
  const handleQmdLinkClick = useCallback(
    (arg: { path: string, anchor: string | null } | { anchor: string }) => {
      if ('path' in arg) {
        onNavigateToDocument(arg.path, arg.anchor);
      } else {
        scrollToAnchor(arg.anchor);
      }
    },
    [scrollToAnchor, onNavigateToDocument]
  );

  const internalPostProcess = useCallback((iframe: HTMLIFrameElement) => {
    postProcessIframe(iframe, {
      currentFilePath,
      onQmdLinkClick: handleQmdLinkClick,
    });
  }, [currentFilePath, handleQmdLinkClick]);

  // Update iframe content when HTML changes
  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe?.contentDocument || !iframe?.contentWindow) return;

    const doc = iframe.contentDocument;
    const win = iframe.contentWindow;

    // Check if this is the first time we're setting content
    // An uninitialized iframe document will have an empty body
    const isFirstLoad = !isInitializedRef.current;

    if (isFirstLoad) {
      // Initial load: write the HTML directly
      doc.open();
      doc.write(html);
      doc.close();

      isInitializedRef.current = true;

      // Post-process after initial load
      internalPostProcess(iframe);
    } else {
      // Subsequent updates: use morphdom to update in place

      // Save scroll position before morphing
      const scrollPos = {
        x: win.scrollX,
        y: win.scrollY,
      };

      // Create a temporary container with the new HTML
      const tempContainer = doc.createElement('html');
      tempContainer.innerHTML = html;

      // Morph the document's documentElement
      // This updates both <head> and <body> efficiently
      morphdom(doc.documentElement, tempContainer);

      // Post-process after morphing
      internalPostProcess(iframe);

      // Restore scroll position
      // Use requestAnimationFrame to ensure DOM has been updated
      requestAnimationFrame(() => {
        win.scrollTo(scrollPos.x, scrollPos.y);
      });
    }
  }, [html, internalPostProcess]);

  // Expose methods via ref
  useImperativeHandle(ref, () => ({
    scrollToLine: (line: number) => {
      const iframe = iframeRef.current;
      const doc = iframe?.contentDocument;
      if (!doc) return;

      const element = findElementForLine(doc, line);
      if (!element) return;

      // Only scroll if element is not already visible
      if (!isElementVisible(element)) {
        element.scrollIntoView({ behavior: 'smooth', block: 'center' });
      }
    },
    getScrollRatio: () => {
      const iframe = iframeRef.current;
      if (!iframe?.contentWindow || !iframe?.contentDocument) return null;

      const iframeWindow = iframe.contentWindow;
      const iframeDoc = iframe.contentDocument;

      // Calculate preview scroll ratio (0 = top, 1 = bottom)
      const previewScrollY = iframeWindow.scrollY;
      const previewScrollHeight = iframeDoc.documentElement.scrollHeight;
      const previewViewportHeight = iframeWindow.innerHeight;
      const previewMaxScroll = previewScrollHeight - previewViewportHeight;

      // Avoid division by zero for short documents
      if (previewMaxScroll <= 0) return 0;

      return previewScrollY / previewMaxScroll;
    },
    setScrollRatio: (ratio: number) => {
      const iframe = iframeRef.current;
      if (!iframe?.contentWindow || !iframe?.contentDocument) return;
      const maxScroll = iframe.contentDocument.documentElement.scrollHeight - iframe.contentWindow.innerHeight;
      if (maxScroll > 0) {
        iframe.contentWindow.scrollTo({ top: ratio * maxScroll });
      }
    },
    setSelection: (startPos: SourceLocation, endPos: SourceLocation) => {
      const iframe = iframeRef.current;
      const doc = iframe?.contentDocument;
      if (!doc) return;

      // Find the most specific (smallest range) elements for start and end positions
      // Now considering both line AND column for position matching
      const elements = doc.querySelectorAll('span[data-loc]');
      let startElement: HTMLElement | null = null;
      let startLoc: SourceLocation | null = null;
      let startRangeSize = Infinity;
      let endElement: HTMLElement | null = null;
      let endLoc: SourceLocation | null = null;
      let endRangeSize = Infinity;

      for (const element of elements) {
        const dataLoc = element.getAttribute('data-loc');
        if (!dataLoc) continue;
        if (element.firstChild?.nodeType !== Node.TEXT_NODE) continue;

        const loc = parseDataLoc(dataLoc);
        if (loc === null) continue;

        // Check if this element contains the start position (considering both line and column)
        if (isPositionAfterOrAt(startPos.startLine, startPos.startCol, loc.startLine, loc.startCol) &&
          isPositionBeforeOrAt(startPos.startLine, startPos.startCol, loc.endLine, loc.endCol)) {
          const rangeSize = loc.endLine - loc.startLine;
          // Prefer smaller (more specific) ranges
          if (rangeSize < startRangeSize) {
            startElement = element as HTMLElement;
            startLoc = loc;
            startRangeSize = rangeSize;
          }
        }

        // Check if this element contains the end position (considering both line and column)
        if (isPositionAfterOrAt(endPos.endLine, endPos.endCol, loc.startLine, loc.startCol) &&
          isPositionBeforeOrAt(endPos.endLine, endPos.endCol, loc.endLine, loc.endCol)) {
          const rangeSize = loc.endLine - loc.startLine;
          // Prefer smaller (more specific) ranges
          if (rangeSize < endRangeSize) {
            endElement = element as HTMLElement;
            endLoc = loc;
            endRangeSize = rangeSize;
          }
        }
      }

      // If we couldn't find matching elements, return
      if (!startElement || !endElement || !startLoc || !endLoc) {
        console.log('Could not find elements for selection', { startPos, endPos });
        return;
      }

      // Calculate the approximate text offsets within the elements
      const startInfo = {
        textNode: startElement.firstChild!,
        offset: startPos.startCol - startLoc.startCol
      }
      const endInfo = {
        textNode: endElement.firstChild!,
        offset: endPos.startCol - endLoc.startCol
      }

      console.log(startElement)

      // Create a range and set it as the document selection
      const selection = doc.getSelection();
      if (!selection) return;

      try {
        const range = doc.createRange();
        range.setStart(startInfo.textNode, startInfo.offset);
        range.setEnd(endInfo.textNode, endInfo.offset);

        selection.removeAllRanges();
        selection.addRange(range);
      } catch (e) {
        console.error('could not set selection', startInfo, endInfo)
        return
      }
    },
    clearSelection: () => {
      const iframe = iframeRef.current;
      const doc = iframe?.contentDocument;
      if (!doc) return;

      const selection = doc.getSelection();
      if (!selection) return;

      selection.removeAllRanges();
    },
  }), []);

  // Set up event listeners on iframe
  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe?.contentWindow || !iframe?.contentDocument) return;

    const handleScroll = () => {
      onScroll?.();
    };

    const handleClick = () => {
      onClick?.();
    };

    const handleSelectionChange = () => {
      if (!onSelectionChange) return;

      const doc = iframe.contentDocument;
      if (!doc) return;

      const selection = doc.getSelection();
      if (!selection || selection.rangeCount === 0) return;

      // Get anchor and focus nodes with their offsets
      const anchorNode = selection.anchorNode;
      const focusNode = selection.focusNode;
      const anchorOffset = selection.anchorOffset;
      const focusOffset = selection.focusOffset;

      if (anchorNode?.nodeType === Node.TEXT_NODE && focusNode?.nodeType === Node.TEXT_NODE) {
        if (anchorNode.parentElement?.tagName !== 'SPAN' || focusNode.parentElement?.tagName !== 'SPAN') return;
        const anchorLoc = parseDataLoc(anchorNode.parentElement?.getAttribute('data-loc')!);
        const focusLoc = parseDataLoc(focusNode.parentElement?.getAttribute('data-loc')!);
        if (anchorLoc === null || focusLoc === null) return;

        const start = addOffsetToPosition(qmdContent, anchorLoc.startLine, anchorLoc.startCol, anchorOffset)
        const end = addOffsetToPosition(qmdContent, focusLoc.startLine, focusLoc.startCol, focusOffset)
        if (start === null || end === null) return;

        onSelectionChange({
          startCol: start.col,
          startLine: start.row,
          endCol: 0, // 0s and fileId don't need to be set, but I don't want to upset typescript
          endLine: 0,
          fileId: anchorLoc.fileId
        }, {
          startCol: 0, // 0s and fileId don't need to be set, but I don't want to upset typescript
          startLine: 0,
          endCol: end.col,
          endLine: end.row,
          fileId: anchorLoc.fileId
        });
      }
    };

    // Listen to scroll on the iframe's content window
    iframe.contentWindow.addEventListener('scroll', handleScroll, { passive: true });
    // Listen to click on the iframe's document
    iframe.contentDocument.addEventListener('click', handleClick);
    // Listen to selectionchange on the iframe's document
    iframe.contentDocument.addEventListener('selectionchange', handleSelectionChange);

    return () => {
      iframe.contentWindow?.removeEventListener('scroll', handleScroll);
      iframe.contentDocument?.removeEventListener('click', handleClick);
      iframe.contentDocument?.removeEventListener('selectionchange', handleSelectionChange);
    };
  }, [onScroll, onClick, onSelectionChange]);

  return (
    <iframe
      ref={iframeRef}
      title="Preview"
      sandbox={'allow-same-origin allow-popups'}
      className="preview-active"
    />
  );
}

export default MorphIframe;

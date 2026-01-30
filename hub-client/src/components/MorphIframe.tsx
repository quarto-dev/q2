import { useRef, useEffect, useCallback, useImperativeHandle } from 'react';
import type { Ref } from 'react';
import morphdom from 'morphdom';
import { postProcessIframe } from '../utils/iframePostProcessor';

// Methods exposed via ref
export interface MorphIframeHandle {
  scrollToLine: (line: number) => void;
  getScrollRatio: () => number | null;
  setSelection: (startPos: SourceLocation, endPos: SourceLocation) => void;
  clearSelection: () => void;
}

interface MorphIframeProps {
  // HTML content to render - component handles morphing automatically
  html: string;
  // Current file path for resolving relative links
  currentFilePath: string;
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
 * Get the first text node within an element (depth-first search).
 */
function getFirstTextNode(element: Node): Text | null {
  if (element.nodeType === Node.TEXT_NODE) {
    return element as Text;
  }

  for (const child of element.childNodes) {
    const textNode = getFirstTextNode(child);
    if (textNode) return textNode;
  }

  return null;
}

/**
 * Get the last text node within an element (depth-first search, reverse order).
 */
function getLastTextNode(element: Node): Text | null {
  if (element.nodeType === Node.TEXT_NODE) {
    return element as Text;
  }

  // Traverse children in reverse order
  const children = Array.from(element.childNodes);
  for (let i = children.length - 1; i >= 0; i--) {
    const textNode = getLastTextNode(children[i]);
    if (textNode) return textNode;
  }

  return null;
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
 * Calculate approximate text offset within an element for a given source position.
 * This is a heuristic that assumes uniform character distribution.
 */
function calculateOffsetInElement(
  targetLine: number,
  targetCol: number,
  element: HTMLElement,
  loc: SourceLocation
): { textNode: Text, offset: number } | null {
  // Get all text content from the element
  const textContent = element.textContent || '';
  const textLength = textContent.length;

  if (textLength === 0) return null;

  // Calculate the "progress" through the source range
  // This is a simplified heuristic that doesn't account for markdown syntax removal
  const sourceStartOffset = (loc.startLine - 1) * 1000 + loc.startCol; // Arbitrary line length
  const sourceEndOffset = (loc.endLine - 1) * 1000 + loc.endCol;
  const targetOffset = (targetLine - 1) * 1000 + targetCol;

  const sourceLength = sourceEndOffset - sourceStartOffset;
  const relativeOffset = targetOffset - sourceStartOffset;

  // Calculate the approximate character offset in the rendered text
  const progress = Math.max(0, Math.min(1, relativeOffset / sourceLength));
  const approximateOffset = Math.floor(progress * textLength);

  // Find the text node at this offset
  let currentOffset = 0;
  const walker = document.createTreeWalker(
    element,
    NodeFilter.SHOW_TEXT,
    null
  );

  let textNode: Text | null = null;
  let offsetInNode = 0;

  while (textNode = walker.nextNode() as Text) {
    const nodeLength = textNode.textContent?.length || 0;
    if (currentOffset + nodeLength >= approximateOffset) {
      offsetInNode = approximateOffset - currentOffset;
      return { textNode, offset: Math.max(0, Math.min(nodeLength, offsetInNode)) };
    }
    currentOffset += nodeLength;
  }

  // Fallback: return the last text node with offset at the end
  const lastTextNode = getLastTextNode(element);
  if (lastTextNode) {
    return { textNode: lastTextNode, offset: lastTextNode.textContent?.length || 0 };
  }

  return null;
}

/**
 * Calculate approximate source position for a given text node and offset within it.
 * This is the reverse of calculateOffsetInElement - it maps from rendered DOM position
 * back to source position. Uses the same heuristic assumption of uniform character distribution.
 */
function calculateSourcePositionFromOffset(
  textNode: Node,
  offsetInTextNode: number,
  element: HTMLElement,
  loc: SourceLocation
): { line: number, col: number } | null {
  // Get all text content from the element
  const textContent = element.textContent || '';
  const textLength = textContent.length;

  if (textLength === 0) return null;

  // Find the absolute offset of the text node within the element
  let absoluteOffset = 0;
  const walker = document.createTreeWalker(
    element,
    NodeFilter.SHOW_TEXT,
    null
  );

  let currentNode: Text | null = null;
  let found = false;

  while (currentNode = walker.nextNode() as Text) {
    if (currentNode === textNode) {
      absoluteOffset += offsetInTextNode;
      found = true;
      break;
    }
    absoluteOffset += currentNode.textContent?.length || 0;
  }

  if (!found) return null;

  // Calculate the progress through the rendered text
  const progress = textLength > 0 ? absoluteOffset / textLength : 0;

  // Map this progress back to the source range
  // This is the inverse of the calculation in calculateOffsetInElement
  const sourceStartOffset = (loc.startLine - 1) * 1000 + loc.startCol;
  const sourceEndOffset = (loc.endLine - 1) * 1000 + loc.endCol;
  const sourceLength = sourceEndOffset - sourceStartOffset;

  const targetSourceOffset = sourceStartOffset + Math.floor(progress * sourceLength);

  // Convert back to line and column
  const line = Math.floor(targetSourceOffset / 1000) + 1;
  const col = (targetSourceOffset % 1000);

  return { line, col };
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
    setSelection: (startPos: SourceLocation, endPos: SourceLocation) => {
      const iframe = iframeRef.current;
      const doc = iframe?.contentDocument;
      if (!doc) return;

      // Find the most specific (smallest range) elements for start and end positions
      // Now considering both line AND column for position matching
      const elements = doc.querySelectorAll('[data-loc]');
      let startElement: HTMLElement | null = null;
      let startLoc: SourceLocation | null = null;
      let startRangeSize = Infinity;
      let endElement: HTMLElement | null = null;
      let endLoc: SourceLocation | null = null;
      let endRangeSize = Infinity;

      for (const element of elements) {
        const dataLoc = element.getAttribute('data-loc');
        if (!dataLoc) continue;

        const loc = parseDataLoc(dataLoc);
        if (!loc) continue;

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

      // Create a range and set it as the document selection
      const selection = doc.getSelection();
      if (!selection) return;

      const range = doc.createRange();

      // Calculate the approximate text offsets within the elements
      const startInfo = calculateOffsetInElement(startPos.startLine, startPos.startCol, startElement, startLoc);
      const endInfo = calculateOffsetInElement(endPos.endLine, endPos.endCol, endElement, endLoc);

      if (startInfo && endInfo) {
        try {
          range.setStart(startInfo.textNode, startInfo.offset);
          range.setEnd(endInfo.textNode, endInfo.offset);
        } catch (e) {
          console.error('Error setting selection range:', e);
          return;
        }
      } else {
        // Fallback to selecting from the first text node to the last text node
        const startTextNode = getFirstTextNode(startElement);
        const endTextNode = getLastTextNode(endElement);

        if (startTextNode && endTextNode) {
          range.setStart(startTextNode, 0);
          range.setEnd(endTextNode, endTextNode.textContent?.length || 0);
        } else {
          // Final fallback to selecting the entire elements
          range.setStart(startElement, 0);
          range.setEnd(endElement, endElement.childNodes.length);
        }
      }

      selection.removeAllRanges();
      selection.addRange(range);
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

      if (!anchorNode || !focusNode) return;

      // Find closest element with data-loc for anchor (start of selection)
      const anchorElement = anchorNode.nodeType === Node.ELEMENT_NODE
        ? (anchorNode as Element).closest('[data-loc]')
        : anchorNode.parentElement?.closest('[data-loc]');

      // Find closest element with data-loc for focus (end of selection)
      const focusElement = focusNode.nodeType === Node.ELEMENT_NODE
        ? (focusNode as Element).closest('[data-loc]')
        : focusNode.parentElement?.closest('[data-loc]');

      if (!anchorElement || !focusElement) return;

      // Parse data-loc attributes
      const anchorDataLoc = anchorElement.getAttribute('data-loc');
      const focusDataLoc = focusElement.getAttribute('data-loc');

      if (!anchorDataLoc || !focusDataLoc) return;

      const anchorLoc = parseDataLoc(anchorDataLoc);
      const focusLoc = parseDataLoc(focusDataLoc);

      if (!anchorLoc || !focusLoc) return;

      // Calculate more precise positions based on the text offset within the nodes
      let startPos: SourceLocation | null = anchorLoc;
      let endPos: SourceLocation | null = focusLoc;

      // Try to refine the anchor position using the text offset
      if (anchorNode.nodeType === Node.TEXT_NODE && anchorOffset !== focusOffset) {
        const refinedAnchor = calculateSourcePositionFromOffset(
          anchorNode,
          anchorOffset,
          anchorElement as HTMLElement,
          anchorLoc
        );
        if (refinedAnchor) {
          startPos = {
            ...anchorLoc,
            startLine: refinedAnchor.line,
            startCol: refinedAnchor.col,
            endLine: refinedAnchor.line,
            endCol: refinedAnchor.col,
          };
        }
      }

      // Try to refine the focus position using the text offset
      if (focusNode.nodeType === Node.TEXT_NODE) {
        const refinedFocus = calculateSourcePositionFromOffset(
          focusNode,
          focusOffset,
          focusElement as HTMLElement,
          focusLoc
        );
        if (refinedFocus) {
          endPos = {
            ...focusLoc,
            startLine: refinedFocus.line,
            startCol: refinedFocus.col,
            endLine: refinedFocus.line,
            endCol: refinedFocus.col,
          };
        }
      }

      onSelectionChange(startPos, endPos);
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

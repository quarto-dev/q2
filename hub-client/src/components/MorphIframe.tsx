import { useRef, useEffect, useCallback, useImperativeHandle } from 'react';
import type { Ref } from 'react';
import morphdom from 'morphdom';
import { postProcessIframe } from '../utils/iframePostProcessor';

// Methods exposed via ref
export interface MorphIframeHandle {
  scrollToLine: (line: number) => void;
  getScrollRatio: () => number | null;
  setSelection: (startPos: SourceLocation, endPos: SourceLocation) => void;
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
      const elements = doc.querySelectorAll('[data-loc]');
      let startElement: HTMLElement | null = null;
      let startRangeSize = Infinity;
      let endElement: HTMLElement | null = null;
      let endRangeSize = Infinity;

      for (const element of elements) {
        const dataLoc = element.getAttribute('data-loc');
        if (!dataLoc) continue;

        const loc = parseDataLoc(dataLoc);
        if (!loc) continue;

        // Check if this element contains the start position
        if (startPos.startLine >= loc.startLine && startPos.startLine <= loc.endLine) {
          const rangeSize = loc.endLine - loc.startLine;
          // Prefer smaller (more specific) ranges
          if (rangeSize < startRangeSize) {
            startElement = element as HTMLElement;
            startRangeSize = rangeSize;
          }
        }

        // Check if this element contains the end position
        if (endPos.endLine >= loc.startLine && endPos.endLine <= loc.endLine) {
          const rangeSize = loc.endLine - loc.startLine;
          // Prefer smaller (more specific) ranges
          if (rangeSize < endRangeSize) {
            endElement = element as HTMLElement;
            endRangeSize = rangeSize;
          }
        }
      }

      // If we couldn't find matching elements, return
      if (!startElement || !endElement) {
        console.log('Could not find elements for selection', { startPos, endPos });
        return;
      }

      // Create a range and set it as the document selection
      const selection = doc.getSelection();
      if (!selection) return;

      const range = doc.createRange();

      // Try to set the range to the first text node in startElement
      // and the last text node in endElement for more precise selection
      const startTextNode = getFirstTextNode(startElement);
      const endTextNode = getLastTextNode(endElement);

      if (startTextNode && endTextNode) {
        range.setStart(startTextNode, 0);
        range.setEnd(endTextNode, endTextNode.textContent?.length || 0);
      } else {
        // Fallback to selecting the entire elements
        range.setStart(startElement, 0);
        range.setEnd(endElement, endElement.childNodes.length);
      }

      selection.removeAllRanges();
      selection.addRange(range);
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

      // Get anchor and focus nodes
      const anchorNode = selection.anchorNode;
      const focusNode = selection.focusNode;

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

      const startPos = parseDataLoc(anchorDataLoc);
      const endPos = parseDataLoc(focusDataLoc);

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

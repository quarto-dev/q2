import { useState, useRef, useEffect, useCallback, useImperativeHandle } from 'react';
import type { Ref } from 'react';
import { postProcessIframe } from '../utils/iframePostProcessor';

// Methods exposed via ref
export interface DoubleBufferedIframeHandle {
  scrollToLine: (line: number) => void;
  getScrollRatio: () => number | null;
}

interface DoubleBufferedIframeProps {
  // HTML content to render - component handles buffering automatically
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
  // Ref to expose imperative methods
  ref: Ref<DoubleBufferedIframeHandle>;
}

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

/**
 * Double-buffered iframe component for flicker-free updates.
 *
 * Maintains two iframes - one visible, one hidden. When new HTML arrives:
 * 1. Loads HTML into the hidden iframe
 * 2. Waits for it to finish loading
 * 3. Calls onBeforeSwap for post-processing (CSS, link handlers, etc.)
 * 4. Swaps to make hidden iframe visible
 * 5. Calls onAfterSwap to notify parent
 *
 * This prevents the visible iframe from flickering during updates.
 */
function DoubleBufferedIframe({
  html,
  currentFilePath,
  onNavigateToDocument,
  onScroll,
  onClick,
  ref,
}: DoubleBufferedIframeProps) {
  const iframeARef = useRef<HTMLIFrameElement>(null);
  const iframeBRef = useRef<HTMLIFrameElement>(null);
  const [activeIframe, setActiveIframe] = useState<'A' | 'B'>('A');
  const [iframeAHtml, setIframeAHtml] = useState<string>('');
  const [iframeBHtml, setIframeBHtml] = useState<string>('');
  const [swapPending, setSwapPending] = useState(false);

  // Get refs for currently active and inactive iframes
  const getIframeRefs = useCallback(() => {
    const active = activeIframe === 'A' ? iframeARef : iframeBRef;
    const inactive = activeIframe === 'A' ? iframeBRef : iframeARef;
    return { active, inactive };
  }, [activeIframe]);

  // Scroll the preview to an anchor element
  const scrollToAnchor = useCallback((anchor: string) => {
    const { active } = getIframeRefs();
    const doc = active.current?.contentDocument;
    if (!doc) return;

    const element = doc.getElementById(anchor);
    if (element) {
      element.scrollIntoView({ behavior: 'instant', block: 'start' });
    }
  }, [getIframeRefs]);

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

  // When new HTML arrives, load it into the inactive iframe and mark swap as pending
  useEffect(() => {
    // Add unique timestamp to ensure srcDoc always changes, forcing onLoad to fire
    // Without this, if HTML is identical to what's already there, React won't update
    // the DOM and onLoad won't fire, breaking the swap mechanism
    const uniqueHtml = html + `<!-- render-${Date.now()} -->`;

    if (activeIframe === 'A') {
      setIframeBHtml(uniqueHtml);
    } else {
      setIframeAHtml(uniqueHtml);
    }
    setSwapPending(true);
  }, [html]);

  // Handler for when inactive iframe finishes loading
  const handleInactiveLoad = useCallback(() => {
    if (!swapPending) return;

    const { active, inactive } = getIframeRefs();

    // Save scroll position from active iframe
    let scrollPos: { x: number; y: number } | null = null;
    if (active.current?.contentWindow) {
      scrollPos = {
        x: active.current.contentWindow.scrollX,
        y: active.current.contentWindow.scrollY,
      };
    }

    // Post-process the inactive iframe BEFORE swapping (critical for CSS, link handlers, etc.)
    if (inactive.current) {
      internalPostProcess(inactive.current);
    }

    // Use requestAnimationFrame to ensure the browser has processed the CSS
    // before swapping iframes. This prevents flash of unstyled content (FOUC)
    // that can occur when CSS is applied via data URI but hasn't been parsed yet.
    requestAnimationFrame(() => {
      // Swap: make inactive become active
      setActiveIframe((prev) => (prev === 'A' ? 'B' : 'A'));
      setSwapPending(false);

      // Restore scroll position after swap (with slight delay for React to re-render)
      if (scrollPos) {
        setTimeout(() => {
          const nowActive = inactive.current;
          if (nowActive?.contentWindow) {
            nowActive.contentWindow.scrollTo(scrollPos!.x, scrollPos!.y);
          }
        }, 0);
      }
    });
  }, [swapPending, getIframeRefs, internalPostProcess]);

  // Handler for when active iframe loads (initial load only)
  const handleActiveLoad = useCallback(() => {
    // For initial load, also do post-processing
    const { active } = getIframeRefs();
    if (active.current) {
      internalPostProcess(active.current);
    }
  }, [getIframeRefs, internalPostProcess]);

  // Expose methods via ref
  useImperativeHandle(ref, () => ({
    scrollToLine: (line: number) => {
      const { active } = getIframeRefs();
      const doc = active.current?.contentDocument;
      if (!doc) return;

      const element = findElementForLine(doc, line);
      if (!element) return;

      // Only scroll if element is not already visible
      if (!isElementVisible(element)) {
        element.scrollIntoView({ behavior: 'smooth', block: 'center' });
      }
    },
    getScrollRatio: () => {
      const { active } = getIframeRefs();
      const iframe = active.current;
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
  }), [getIframeRefs]);

  // Set up event listeners on active iframe
  useEffect(() => {
    const { active } = getIframeRefs();
    const iframe = active.current;
    if (!iframe?.contentWindow || !iframe?.contentDocument) return;

    const handleScroll = () => {
      onScroll?.();
    };

    const handleClick = () => {
      onClick?.();
    };

    // Listen to scroll on the iframe's content window
    iframe.contentWindow.addEventListener('scroll', handleScroll, { passive: true });
    // Listen to click on the iframe's document
    iframe.contentDocument.addEventListener('click', handleClick);

    return () => {
      iframe.contentWindow?.removeEventListener('scroll', handleScroll);
      iframe.contentDocument?.removeEventListener('click', handleClick);
    };
  }, [activeIframe, onScroll, onClick, getIframeRefs]);

  return (
    <>
      <iframe
        ref={iframeARef}
        srcDoc={iframeAHtml}
        title={`A`}
        sandbox={'allow-same-origin allow-popups'}
        onLoad={activeIframe === 'A' ? handleActiveLoad : handleInactiveLoad}
        className={activeIframe === 'A' ? 'preview-active' : 'preview-hidden'}
      />
      <iframe
        ref={iframeBRef}
        srcDoc={iframeBHtml}
        title={`B`}
        sandbox={'allow-same-origin allow-popups'}
        onLoad={activeIframe === 'B' ? handleActiveLoad : handleInactiveLoad}
        className={activeIframe === 'B' ? 'preview-active' : 'preview-hidden'}
      />
    </>
  );
}

export default DoubleBufferedIframe;

import { useState, useRef, useEffect, useCallback } from 'react';
import { postProcessIframe } from '../utils/iframePostProcessor';

interface DoubleBufferedIframeProps {
  // HTML content to render - component handles buffering automatically
  html: string;
  // Current file path for resolving relative links
  currentFilePath: string;
  // Callback when user navigates to a different document (with optional anchor)
  // Parent (Preview) handles file lookup and switching
  onNavigateToDocument?: (targetPath: string, anchor: string | null) => void;
  // Called AFTER swap completes (for tracking load count, etc.)
  onAfterSwap?: () => void;
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
export default function DoubleBufferedIframe({
  html,
  currentFilePath,
  onNavigateToDocument,
  onAfterSwap,
}: DoubleBufferedIframeProps) {
  const iframeARef = useRef<HTMLIFrameElement>(null);
  const iframeBRef = useRef<HTMLIFrameElement>(null);
  const [activeIframe, setActiveIframe] = useState<'A' | 'B'>('A');
  const [iframeAHtml, setIframeAHtml] = useState<string>('');
  const [iframeBHtml, setIframeBHtml] = useState<string>('');
  const [swapPending, setSwapPending] = useState(false);

  // Track which iframe is active in a ref for use in callbacks
  const activeIframeRef = useRef<'A' | 'B'>('A');
  useEffect(() => {
    activeIframeRef.current = activeIframe;
  }, [activeIframe]);

  // Get refs for currently active and inactive iframes
  const getIframeRefs = useCallback(() => {
    const active = activeIframe === 'A' ? iframeARef : iframeBRef;
    const inactive = activeIframe === 'A' ? iframeBRef : iframeARef;
    return { active, inactive };
  }, [activeIframe]);

  // Get the currently active iframe element
  const getActiveIframe = useCallback(() => {
    return activeIframe === 'A' ? iframeARef.current : iframeBRef.current;
  }, [activeIframe]);

  // Scroll the preview to an anchor element
  const scrollToAnchor = useCallback((anchor: string) => {
    const activeIframeEl = getActiveIframe();
    const doc = activeIframeEl?.contentDocument;
    if (!doc) return;

    const element = doc.getElementById(anchor);
    if (element) {
      element.scrollIntoView({ behavior: 'instant', block: 'start' });
    }
  }, [getActiveIframe]);

  // Handler for .qmd link clicks and anchor clicks in the preview
  const handleQmdLinkClick = useCallback(
    (targetPath: string | null, anchor: string | null) => {
      // Case 1: Same-document anchor only (e.g., #section)
      if (!targetPath && anchor) {
        scrollToAnchor(anchor);
        return;
      }

      // Case 2: Link to a different document (with or without anchor)
      if (targetPath) {
        onNavigateToDocument?.(targetPath, anchor);
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

    if (activeIframeRef.current === 'A') {
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

    // Notify parent that swap completed
    onAfterSwap?.();
  }, [swapPending, getIframeRefs, internalPostProcess, onAfterSwap]);

  // Handler for when active iframe loads (initial load only)
  const handleActiveLoad = useCallback(() => {
    // For initial load, also do post-processing
    const { active } = getIframeRefs();
    if (active.current) {
      internalPostProcess(active.current);
    }
    onAfterSwap?.();
  }, [getIframeRefs, internalPostProcess, onAfterSwap]);

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

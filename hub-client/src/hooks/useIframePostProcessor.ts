/**
 * React hook for post-processing iframe content after render.
 *
 * Follows React-idiomatic patterns:
 * - onLoad handler just updates state (doesn't do work)
 * - useEffect reacts to state and runs post-processor
 * - Encapsulates all post-processing logic
 */

import { useCallback, useEffect, useState, useMemo } from 'react';
import type { RefObject } from 'react';
import { postProcessIframe } from '../utils/iframePostProcessor';
import type { PostProcessOptions } from '../utils/iframePostProcessor';

/**
 * Hook for post-processing iframe content after render.
 *
 * @param iframeRef - Reference to the iframe element
 * @param options - Post-processing options (currentFilePath, onQmdLinkClick)
 * @returns Object containing handleLoad callback to attach to iframe's onLoad
 */
export function useIframePostProcessor(
  iframeRef: RefObject<HTMLIFrameElement | null>,
  options: PostProcessOptions
) {
  // Track load events as state changes
  const [loadCount, setLoadCount] = useState(0);

  // Memoize options to prevent unnecessary effect runs
  const memoizedOptions = useMemo(
    () => ({
      currentFilePath: options.currentFilePath,
      onQmdLinkClick: options.onQmdLinkClick,
    }),
    [options.currentFilePath, options.onQmdLinkClick]
  );

  // Handler just signals "iframe loaded" - no work here
  const handleLoad = useCallback(() => {
    setLoadCount((n) => n + 1);
  }, []);

  // Work happens in useEffect, reacting to state
  useEffect(() => {
    if (loadCount > 0 && iframeRef.current?.contentDocument) {
      postProcessIframe(iframeRef.current, memoizedOptions);
    }
  }, [loadCount, iframeRef, memoizedOptions]);

  return { handleLoad };
}

/**
 * useSlideThumbnails Hook
 *
 * Generates thumbnail images for presentation slides.
 * Each thumbnail is 128x64 pixels (maintaining 3:2 aspect ratio).
 */

import { useState, useEffect, useCallback, useRef } from 'react';
import ReactDOM from 'react-dom/client';
import html2canvas from 'html2canvas';
import type { Symbol } from '../types/intelligence';
import { parseSlides, renderSlide, type PandocAST } from '../components/ReactAstSlideRenderer';

/**
 * Map from symbol line number to thumbnail data URL.
 */
export type ThumbnailMap = Map<number, string>;

interface UseSlideThumbnailsOptions {
  /** Pandoc AST JSON string. */
  astJson: string | null;
  /** Document symbols (headers) to generate thumbnails for. */
  symbols: Symbol[];
  /** Whether the preview is ready for thumbnail capture. */
  previewReady: boolean;
  /** Trigger value that changes when preview content updates. */
  contentVersion: number;
}


/**
 * Generate thumbnails for presentation slides.
 *
 * Renders each slide off-screen and captures it as a thumbnail.
 * Thumbnails are captured at 128x64 pixels (3:2 aspect ratio).
 */
export function useSlideThumbnails({
  astJson,
  symbols,
  previewReady,
  contentVersion,
}: UseSlideThumbnailsOptions): ThumbnailMap {
  const [thumbnails, setThumbnails] = useState<ThumbnailMap>(new Map());
  const captureInProgressRef = useRef(false);
  const debounceTimeoutRef = useRef<number | null>(null);

  const captureThumbnails = useCallback(async () => {
    // Avoid overlapping capture operations
    if (captureInProgressRef.current) return;
    if (!previewReady) return;
    if (!astJson) return;
    if (symbols.length === 0) return;

    captureInProgressRef.current = true;

    try {
      const newThumbnails = new Map<number, string>();

      // Parse AST to get slides
      let ast: PandocAST;
      try {
        ast = JSON.parse(astJson);
      } catch (err) {
        console.error('Failed to parse AST for thumbnails:', err);
        return;
      }

      const slides = parseSlides(ast);

      // Filter symbols to only headers (those that create slides)
      // Headers use SymbolKind 'string' in our LSP
      // The symbols array contains top-level symbols only (h1/h2), with h3+ nested as children
      const slideHeaders = symbols.filter(s => s.kind === 'string');

      // Match slides to headers by index (structural position)
      let headerIndex = 0;
      for (let slideIndex = 0; slideIndex < slides.length; slideIndex++) {
        const slide = slides[slideIndex];

        // Skip title slides (they don't correspond to document sections)
        if (slide.type === 'title') {
          continue;
        }

        // Match this content slide to the next header by index
        if (headerIndex >= slideHeaders.length) {
          continue;
        }

        const matchingSymbol = slideHeaders[headerIndex];
        headerIndex++;

        // Create a temporary container for rendering this slide
        const container = document.createElement('div');
        container.style.position = 'absolute';
        container.style.left = '-10000px';
        container.style.top = '-10000px';
        container.style.width = '1050px';
        container.style.height = '700px';
        container.style.backgroundColor = 'white';
        container.style.boxShadow = '0 0 30px rgba(0,0,0,0.5)';
        document.body.appendChild(container);

        try {
          // Render the slide content using React
          const root = ReactDOM.createRoot(container);

          // Wait for rendering and capture
          await new Promise<void>((resolve) => {
            root.render(<div>{renderSlide(slide)}</div>);

            // Give React time to render
            setTimeout(async () => {
              try {
                // Capture the container
                const canvas = await html2canvas(container, {
                  backgroundColor: '#ffffff',
                  useCORS: true,
                  logging: false,
                  scale: 0.3, // Lower scale for performance
                  width: 1050,
                  height: 700,
                });

                // Resize to thumbnail size (128x64 maintains 3:2 ratio)
                const thumbnailCanvas = document.createElement('canvas');
                thumbnailCanvas.width = 128;
                thumbnailCanvas.height = 64;
                const ctx = thumbnailCanvas.getContext('2d');
                if (ctx) {
                  ctx.drawImage(canvas, 0, 0, 128, 64);
                  const dataUrl = thumbnailCanvas.toDataURL('image/png');
                  newThumbnails.set(matchingSymbol.range.start.line, dataUrl);
                }
              } catch (error) {
                console.error(`Failed to capture thumbnail for slide ${slideIndex} ("${matchingSymbol.name}"):`, error);
              } finally {
                root.unmount();
                resolve();
              }
            }, 50); // Short delay for React to render
          });
        } finally {
          // Clean up the temporary container
          document.body.removeChild(container);
        }
      }

      setThumbnails(newThumbnails);
    } finally {
      captureInProgressRef.current = false;
    }
  }, [astJson, symbols, previewReady]);

  // Debounced capture thumbnails when content changes
  useEffect(() => {
    if (debounceTimeoutRef.current) {
      clearTimeout(debounceTimeoutRef.current);
    }

    debounceTimeoutRef.current = window.setTimeout(() => {
      captureThumbnails();
    }, 100);

    return () => {
      if (debounceTimeoutRef.current) {
        clearTimeout(debounceTimeoutRef.current);
      }
    };
  }, [contentVersion, captureThumbnails]);

  return thumbnails;
}

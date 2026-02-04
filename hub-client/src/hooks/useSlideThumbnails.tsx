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
import { parseSlides, renderSlide, type PandocAST, type Slide } from '../components/ReactAstSlideRenderer';

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
 * Extract header text from a slide for matching with symbols.
 */
function getSlideHeaderText(slide: Slide): string | null {
  if (slide.type === 'title' && slide.title) {
    return slide.title;
  }

  // For content slides, find the first header block
  for (const block of slide.blocks) {
    if (block.t === 'Header') {
      const [, , inlines] = block.c as [number, [string, string[], [string, string][]], any[]];
      return inlines
        .map((inline: any) => {
          if (inline.t === 'Str') return inline.c;
          if (inline.t === 'Space') return ' ';
          return '';
        })
        .join('')
        .trim();
    }
  }

  return null;
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

      // For each slide, render it and capture a thumbnail
      for (const slide of slides) {
        // Skip title slides (they don't correspond to document sections)
        if (slide.type === 'title') continue;

        // Get the header text from this slide
        const headerText = getSlideHeaderText(slide);
        if (!headerText) continue;

        // Find the matching symbol
        const matchingSymbol = symbols.find((s) => s.name === headerText);
        if (!matchingSymbol) continue;

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
                console.error(`Failed to capture thumbnail for "${headerText}":`, error);
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

  // Capture thumbnails when content changes
  useEffect(() => {
    captureThumbnails();
  }, [contentVersion, captureThumbnails]);

  return thumbnails;
}

/**
 * useSectionThumbnails Hook
 *
 * Generates thumbnail images for document sections (h1 headers + content).
 * Each thumbnail is 128x64 pixels.
 */

import { useState, useEffect, useCallback, useRef } from 'react';
import html2canvas from 'html2canvas';
import type { Symbol } from '../types/intelligence';

/**
 * Map from symbol line number to thumbnail data URL.
 */
export type ThumbnailMap = Map<number, string>;

interface UseSectionThumbnailsOptions {
  /** Document symbols (headers) to generate thumbnails for. */
  symbols: Symbol[];
  /** Whether the preview is ready for thumbnail capture. */
  previewReady: boolean;
  /** Trigger value that changes when preview content updates. */
  contentVersion: number;
}

/**
 * Generate thumbnails for document sections.
 *
 * A section is defined as an h1 header plus all content until the next h1.
 * Thumbnails are captured at 128x64 pixels.
 */
export function useSectionThumbnails({
  symbols,
  previewReady,
  contentVersion,
}: UseSectionThumbnailsOptions): ThumbnailMap {
  const [thumbnails, setThumbnails] = useState<ThumbnailMap>(new Map());
  const captureInProgressRef = useRef(false);

  const captureThumbnails = useCallback(async () => {
    // Avoid overlapping capture operations
    if (captureInProgressRef.current) return;
    if (!previewReady) return;
    if (symbols.length === 0) return;

    captureInProgressRef.current = true;

    try {
      const newThumbnails = new Map<number, string>();

      // Find h1 headers in the preview pane
      const previewPane = document.querySelector('.preview-pane');
      if (!previewPane) {
        console.warn('Preview pane not found for thumbnail capture');
        return;
      }

      // Get all h1 elements
      const h1Elements = previewPane.querySelectorAll('h1');

      // Match symbols to h1 elements
      // We'll use the symbol's line number as the key
      const h1Symbols = symbols.filter((s) => s.kind === 'string'); // Headers use 'string' kind

      for (let i = 0; i < h1Elements.length; i++) {
        const h1 = h1Elements[i] as HTMLElement;
        const symbol = h1Symbols[i];

        if (!symbol) continue;

        // Create a temporary container for this section
        const container = document.createElement('div');
        container.style.position = 'absolute';
        container.style.left = '-10000px';
        container.style.top = '-10000px';
        container.style.width = '800px'; // Match typical preview width
        container.style.backgroundColor = 'white';
        container.style.padding = '20px';
        document.body.appendChild(container);

        // Clone the h1 and add it
        const h1Clone = h1.cloneNode(true) as HTMLElement;
        container.appendChild(h1Clone);

        // Find all siblings until the next h1
        let sibling = h1.nextElementSibling;
        while (sibling && sibling.tagName !== 'H1') {
          const siblingClone = sibling.cloneNode(true) as HTMLElement;
          container.appendChild(siblingClone);
          sibling = sibling.nextElementSibling;
        }

        try {
          // Capture the container
          const canvas = await html2canvas(container, {
            backgroundColor: '#ffffff',
            useCORS: true,
            logging: false,
            scale: 0.5, // Reduce resolution for performance
            width: 800,
            windowWidth: 800,
          });

          // Resize to 128x64
          const thumbnailCanvas = document.createElement('canvas');
          thumbnailCanvas.width = 128;
          thumbnailCanvas.height = 64;
          const ctx = thumbnailCanvas.getContext('2d');
          if (ctx) {
            ctx.drawImage(canvas, 0, 0, 128, 64);
            const dataUrl = thumbnailCanvas.toDataURL('image/png');
            newThumbnails.set(symbol.range.start.line, dataUrl);
          }
        } catch (error) {
          console.error('Failed to capture section thumbnail:', error);
        } finally {
          // Clean up the temporary container
          document.body.removeChild(container);
        }
      }

      setThumbnails(newThumbnails);
    } finally {
      captureInProgressRef.current = false;
    }
  }, [symbols, previewReady]);

  // Capture thumbnails when content changes
  useEffect(() => {
    captureThumbnails();
  }, [contentVersion, captureThumbnails]);

  return thumbnails;
}

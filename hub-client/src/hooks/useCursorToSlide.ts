/**
 * useCursorToSlide Hook
 *
 * Maps cursor line numbers to slide indices for cursor-driven slide navigation.
 */

import { useMemo } from 'react';
import type { Symbol } from '../types/intelligence';
import { parseSlides, type PandocAST } from '../components/ReactAstSlideRenderer';

interface SlideMapping {
  /** The starting line (0-based) where this slide begins. */
  startLine: number;
  /** The slide index. */
  slideIndex: number;
}

/**
 * Hook that provides a function to map cursor line numbers to slide indices.
 *
 * @param astJson - The Pandoc AST JSON string
 * @param symbols - Document symbols (headers) from intelligence
 * @returns A function that takes a line number (0-based) and returns the corresponding slide index
 */
export function useCursorToSlide(
  astJson: string | null,
  symbols: Symbol[]
): (line: number) => number {
  const slideMapping = useMemo((): SlideMapping[] => {
    if (!astJson || symbols.length === 0) {
      return [];
    }

    try {
      const ast: PandocAST = JSON.parse(astJson);
      const slides = parseSlides(ast);
      const mappings: SlideMapping[] = [];

      // Filter symbols to only headers (those that create slides)
      // Headers use SymbolKind 'string' in our LSP
      // The symbols array contains top-level symbols only (h1/h2), with h3+ nested as children
      const slideHeaders = symbols.filter(s => s.kind === 'string');

      // Match slides to headers by index (structural position)
      let headerIndex = 0;
      for (let slideIndex = 0; slideIndex < slides.length; slideIndex++) {
        const slide = slides[slideIndex];

        if (slide.type === 'title') {
          // Title slide starts at line 0
          mappings.push({ startLine: 0, slideIndex });
          continue;
        }

        // Match this content slide to the next header by index
        if (headerIndex < slideHeaders.length) {
          const header = slideHeaders[headerIndex];
          mappings.push({
            startLine: header.range.start.line,
            slideIndex,
          });
          headerIndex++;
        }
      }

      // Sort by line number (ascending) - should already be sorted, but just in case
      mappings.sort((a, b) => a.startLine - b.startLine);

      return mappings;
    } catch (err) {
      console.error('Failed to build cursor-to-slide mapping:', err);
      return [];
    }
  }, [astJson, symbols]);

  // Return a function that maps a line number to a slide index
  return useMemo(() => {
    return (line: number): number => {
      if (slideMapping.length === 0) {
        return 0;
      }

      // Find the last slide that starts at or before this line
      let slideIndex = 0;
      for (let i = slideMapping.length - 1; i >= 0; i--) {
        if (slideMapping[i].startLine <= line) {
          slideIndex = slideMapping[i].slideIndex;
          break;
        }
      }

      return slideIndex;
    };
  }, [slideMapping]);
}

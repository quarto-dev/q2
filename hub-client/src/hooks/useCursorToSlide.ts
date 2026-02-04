/**
 * useCursorToSlide Hook
 *
 * Maps cursor line numbers to slide indices for cursor-driven slide navigation.
 */

import { useMemo } from 'react';
import type { Symbol } from '../types/intelligence';
import { parseSlides, type PandocAST, type Slide } from '../components/ReactAstSlideRenderer';

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

      // For each slide, find its corresponding symbol and line number
      for (let slideIndex = 0; slideIndex < slides.length; slideIndex++) {
        const slide = slides[slideIndex];

        if (slide.type === 'title') {
          // Title slide starts at line 0
          mappings.push({ startLine: 0, slideIndex });
          continue;
        }

        // Get the header text from this slide
        const headerText = getSlideHeaderText(slide);
        if (!headerText) continue;

        // Find the matching symbol
        const matchingSymbol = symbols.find((s) => s.name === headerText);
        if (!matchingSymbol) continue;

        // Add mapping from symbol's line to this slide
        mappings.push({
          startLine: matchingSymbol.range.start.line,
          slideIndex,
        });
      }

      // Sort by line number (ascending)
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

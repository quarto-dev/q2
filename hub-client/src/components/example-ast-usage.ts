/**
 * Example: Using parseQmdToAst in hub-client
 *
 * This example demonstrates how to parse QMD content to Pandoc AST
 * for programmatic manipulation or analysis.
 */

import { parseQmdToAst } from '../services/wasmRenderer';

/**
 * Parse a QMD document and extract all heading text
 */
export async function extractHeadings(qmdContent: string): Promise<string[]> {
  // Parse to AST
  const astJson = await parseQmdToAst(qmdContent);
  const ast = JSON.parse(astJson);

  // Extract headings
  const headings: string[] = [];

  for (const block of ast.blocks || []) {
    if (block.t === 'Header') {
      // Header format: { t: "Header", c: [level, attr, inlines] }
      const [_level, _attr, inlines] = block.c;
      const text = extractTextFromInlines(inlines);
      headings.push(text);
    }
  }

  return headings;
}

/**
 * Extract plain text from Pandoc inline elements
 */
function extractTextFromInlines(inlines: any[]): string {
  let text = '';
  for (const inline of inlines) {
    if (inline.t === 'Str') {
      text += inline.c;
    } else if (inline.t === 'Space') {
      text += ' ';
    } else if (inline.c && Array.isArray(inline.c)) {
      // Recursively handle nested inlines (e.g., in Emph, Strong)
      const nested = inline.c.find((item: any) => Array.isArray(item));
      if (nested) {
        text += extractTextFromInlines(nested);
      }
    }
  }
  return text;
}

/**
 * Example: Count blocks by type
 */
export async function analyzeDocument(qmdContent: string) {
  const astJson = await parseQmdToAst(qmdContent);
  const ast = JSON.parse(astJson);

  const blockCounts: Record<string, number> = {};

  for (const block of ast.blocks || []) {
    blockCounts[block.t] = (blockCounts[block.t] || 0) + 1;
  }

  return {
    apiVersion: ast['pandoc-api-version'],
    meta: ast.meta,
    totalBlocks: ast.blocks?.length || 0,
    blockTypes: blockCounts,
  };
}

/**
 * Example usage:
 *
 * const qmd = `# Introduction\n\nThis is a paragraph.\n\n## Subsection\n\nAnother paragraph.`;
 *
 * const headings = await extractHeadings(qmd);
 * console.log(headings); // ["Introduction", "Subsection"]
 *
 * const analysis = await analyzeDocument(qmd);
 * console.log(analysis);
 * // {
 * //   apiVersion: [1, 23, 1],
 * //   meta: {},
 * //   totalBlocks: 4,
 * //   blockTypes: { Header: 2, Para: 2 }
 * // }
 */

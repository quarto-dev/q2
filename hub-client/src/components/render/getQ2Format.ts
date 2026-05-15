import { extractMetaString } from './framework';

/**
 * Extract format string from the parsed AST metadata.
 * Returns null if no format is found or format is not handled by ReactPreview,
 * otherwise returns the format string (e.g., 'q2-slides', 'q2-debug', 'revealjs').
 */
export function getQ2Format(astJson: string): string | null {
  try {
    const ast = JSON.parse(astJson);
    const formatStr = extractMetaString(ast?.meta?.format);
    if (!formatStr) return null;
    // Only return formats handled by ReactPreview
    if (formatStr.startsWith('q2-') || formatStr === 'revealjs') return formatStr;
    return null;
  } catch (err) {
    console.error('[PreviewRouter] Failed to parse AST:', err);
    return null;
  }
}

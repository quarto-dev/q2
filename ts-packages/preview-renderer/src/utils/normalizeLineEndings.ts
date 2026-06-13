/**
 * Normalize CRLF line endings to LF.
 *
 * Applied to sliced source strings (from `sliceBytes`) before comparing
 * with textarea drafts. The textarea HTML spec LF-normalizes its value,
 * so a CRLF block's sliced text must be normalized too for an accurate
 * dirty check.
 *
 * IMPORTANT: Apply ONLY to sliced strings, NEVER to the `content` buffer.
 * The content buffer's byte offsets are CRLF-domain; modifying it would
 * corrupt all subsequent offset-based slicing.
 */
export function normalizeLineEndings(s: string): string {
    return s.replace(/\r\n/g, '\n');
}

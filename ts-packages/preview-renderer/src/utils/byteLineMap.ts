/**
 * Byte-offset ↔ line-number map for q2-preview source positions.
 *
 * The q2 parser reports all source positions as UTF-8 byte offsets. This
 * utility builds a line-start table from a `content` string once, then
 * answers byte→line and line→byte queries in O(log n) time.
 *
 * ## Coordinate conventions (0-based throughout)
 *
 * - **Lines** are 0-based. Line 0 starts at byte offset 0.
 * - **Line boundary:** a `\n` (0x0A) byte ends the current line; the byte
 *   immediately *after* the `\n` begins the next line. A `\r` (0x0D) is an
 *   ordinary byte belonging to the preceding line — there is no special-casing
 *   of `\r\n`. This means CRLF documents work for free: the `\r` belongs to
 *   the line closed by its following `\n`, and the post-`\n` byte is the next
 *   line's start.
 * - **Trailing newline:** a `\n` at the very end of the content creates an
 *   empty final line. For example, `"foo\n"` has lineCount === 2 (line 0 =
 *   "foo\n", line 1 = "").
 * - **`lineOf(offset)`** returns the 0-based line index of the given byte
 *   offset. Offsets at or beyond the last byte (`byteLength`) are clamped to
 *   the last line. Offset 0 is always line 0.
 * - **`lineStart(line)`** returns the byte offset where that 0-based line
 *   begins. `lineStart(0)` is always 0.
 *
 * ## Usage
 *
 * ```ts
 * const map = buildByteLineMap(content);
 * const line = map.lineOf(r0);          // O(log n)
 * const startOffset = map.lineStart(2); // O(1)
 * const total = map.lineCount;
 * ```
 */

const encoder = new TextEncoder();

export interface ByteLineMap {
    /**
     * The total number of lines. Always ≥ 1 (even an empty string has 1 line).
     * A trailing `\n` adds one empty final line, so `"foo\n"` has lineCount === 2.
     */
    readonly lineCount: number;

    /**
     * Return the 0-based line index of the given UTF-8 byte offset.
     *
     * - Offsets within line N's range map to N.
     * - Offsets at or beyond `byteLength` are clamped to `lineCount - 1`.
     * - Offset 0 is always line 0.
     */
    lineOf(byteOffset: number): number;

    /**
     * Return the byte offset where the given 0-based line begins.
     *
     * - `lineStart(0)` is always 0.
     * - Passing a line ≥ lineCount is undefined behaviour; implementations may
     *   clamp or return the last known start.
     */
    lineStart(line: number): number;
}

/**
 * Build a `ByteLineMap` from `content` by encoding it to UTF-8 and scanning
 * for `\n` (0x0A) bytes. The table is computed once and held in the returned
 * object; subsequent `lineOf`/`lineStart` queries run in O(log n) / O(1).
 */
export function buildByteLineMap(content: string): ByteLineMap {
    const bytes = encoder.encode(content);
    const byteLength = bytes.length;

    // `starts[i]` = byte offset of the first byte on line i.
    // Line 0 always starts at offset 0.
    const starts: number[] = [0];

    for (let i = 0; i < byteLength; i++) {
        if (bytes[i] === 0x0a /* \n */) {
            starts.push(i + 1);
        }
    }

    const lineCount = starts.length;

    return {
        lineCount,

        lineOf(byteOffset: number): number {
            // Clamp to [0, byteLength] so callers can pass byteLength safely.
            // Fast path: clamp negatives and zero to line 0.
            if (byteOffset <= 0) return 0;
            if (byteOffset >= byteLength) return lineCount - 1;

            // Binary search: find the largest index i such that starts[i] <= byteOffset.
            let lo = 0;
            let hi = lineCount - 1;
            while (lo < hi) {
                // Bias toward the upper half to find the *last* start ≤ offset.
                const mid = (lo + hi + 1) >> 1;
                if (starts[mid] <= byteOffset) {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            return lo;
        },

        lineStart(line: number): number {
            if (line <= 0) return 0;
            if (line >= lineCount) return starts[lineCount - 1];
            return starts[line];
        },
    };
}

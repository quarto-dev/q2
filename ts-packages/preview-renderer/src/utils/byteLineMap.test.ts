/**
 * Unit tests for `utils/byteLineMap.ts`.
 *
 * All offsets are UTF-8 byte offsets. Line numbers are 0-based throughout.
 *
 * Phase 2.1 — byte→line map primitive.
 */

import { describe, it, expect } from 'vitest';
import { buildByteLineMap } from './byteLineMap';

// ── LF document ─────────────────────────────────────────────────────────────

describe('LF document', () => {
    // "line0\nline1\nline2"
    //  byte positions:
    //  l=0  0-4
    //  \n   5
    //  l=1  6-10
    //  \n   11
    //  l=2  12-16
    const content = 'line0\nline1\nline2';

    it('lineCount is 3', () => {
        const m = buildByteLineMap(content);
        expect(m.lineCount).toBe(3);
    });

    it('lineOf: start of line 0 → 0', () => {
        expect(buildByteLineMap(content).lineOf(0)).toBe(0);
    });

    it('lineOf: middle of line 0 → 0', () => {
        expect(buildByteLineMap(content).lineOf(3)).toBe(0);
    });

    it('lineOf: byte just before first \\n (offset 4) → 0', () => {
        expect(buildByteLineMap(content).lineOf(4)).toBe(0);
    });

    it('lineOf: byte after first \\n (offset 6) → 1', () => {
        expect(buildByteLineMap(content).lineOf(6)).toBe(1);
    });

    it('lineOf: start of line 2 (offset 12) → 2', () => {
        expect(buildByteLineMap(content).lineOf(12)).toBe(2);
    });

    it('lineOf: end of line 2 (last byte, offset 16) → 2', () => {
        expect(buildByteLineMap(content).lineOf(16)).toBe(2);
    });

    it('lineStart round-trips: lineStart(0) = 0', () => {
        const m = buildByteLineMap(content);
        expect(m.lineStart(0)).toBe(0);
    });

    it('lineStart round-trips: lineStart(1) = 6', () => {
        const m = buildByteLineMap(content);
        expect(m.lineStart(1)).toBe(6);
    });

    it('lineStart round-trips: lineStart(2) = 12', () => {
        const m = buildByteLineMap(content);
        expect(m.lineStart(2)).toBe(12);
    });

    it('lineOf(lineStart(i)) === i for all lines', () => {
        const m = buildByteLineMap(content);
        for (let i = 0; i < m.lineCount; i++) {
            expect(m.lineOf(m.lineStart(i))).toBe(i);
        }
    });
});

// ── CRLF document ────────────────────────────────────────────────────────────

describe('CRLF document', () => {
    // "aaa\r\nbbb\r\nccc"
    // Bytes:
    //  a=0x61 at 0,1,2
    //  \r=0x0D at 3  (belongs to line 0 — a \r is an ordinary byte before the \n)
    //  \n=0x0A at 4  (the actual line separator)
    //  b=0x62 at 5,6,7
    //  \r=0x0D at 8
    //  \n=0x0A at 9
    //  c=0x63 at 10,11,12
    const content = 'aaa\r\nbbb\r\nccc';

    it('lineCount is 3', () => {
        expect(buildByteLineMap(content).lineCount).toBe(3);
    });

    it('lineStart(0) = 0', () => {
        expect(buildByteLineMap(content).lineStart(0)).toBe(0);
    });

    it('lineStart(1) = 5 (byte after \\n at 4)', () => {
        expect(buildByteLineMap(content).lineStart(1)).toBe(5);
    });

    it('lineStart(2) = 10 (byte after \\n at 9)', () => {
        expect(buildByteLineMap(content).lineStart(2)).toBe(10);
    });

    it('lineOf(3) = 0: the \\r at offset 3 is on line 0 (current line), not the next', () => {
        // \r is an ordinary byte — it belongs to the line before the \n.
        expect(buildByteLineMap(content).lineOf(3)).toBe(0);
    });

    it('lineOf(4) = 0: the \\n itself is on line 0 (it closes line 0, before the next start)', () => {
        // The \n at offset 4 falls before line 1's start at offset 5,
        // so lineOf(4) == 0 (binary-search: largest line-start ≤ offset).
        expect(buildByteLineMap(content).lineOf(4)).toBe(0);
    });

    it('lineOf(5) = 1: first byte of line 1', () => {
        expect(buildByteLineMap(content).lineOf(5)).toBe(1);
    });

    it('lineOf(8) = 1: the \\r on line 1 is on line 1', () => {
        expect(buildByteLineMap(content).lineOf(8)).toBe(1);
    });

    it('lineOf(10) = 2: first byte of line 2', () => {
        expect(buildByteLineMap(content).lineOf(10)).toBe(2);
    });
});

// ── Trailing newline ─────────────────────────────────────────────────────────

describe('trailing newline', () => {
    // "foo\n"
    // Bytes: f=0, o=1, o=2, \n=3
    // Line 0 starts at 0; line 1 starts at 4 (the empty final line after the \n).
    const content = 'foo\n';

    it('lineCount is 2 (final \\n creates an empty line 1)', () => {
        // A trailing \n means the byte at position 4 (== byteLength) begins line 1.
        // lineCount = number of lines = 2.
        expect(buildByteLineMap(content).lineCount).toBe(2);
    });

    it('lineStart(0) = 0', () => {
        expect(buildByteLineMap(content).lineStart(0)).toBe(0);
    });

    it('lineStart(1) = 4', () => {
        expect(buildByteLineMap(content).lineStart(1)).toBe(4);
    });

    it('lineOf at byteLength (= 4) → last line (1)', () => {
        // Offset == byteLength is on the last line (clamped).
        const m = buildByteLineMap(content);
        const encoder = new TextEncoder();
        const byteLen = encoder.encode(content).length;
        expect(m.lineOf(byteLen)).toBe(m.lineCount - 1);
    });
});

// ── Multi-byte UTF-8 characters ──────────────────────────────────────────────

describe('multi-byte UTF-8 characters', () => {
    // "🎉\nhi"
    // 🎉 is 4 bytes: offsets 0-3
    // \n at offset 4
    // h at offset 5, i at offset 6
    const content = '🎉\nhi';

    it('lineCount is 2', () => {
        expect(buildByteLineMap(content).lineCount).toBe(2);
    });

    it('lineStart(0) = 0', () => {
        expect(buildByteLineMap(content).lineStart(0)).toBe(0);
    });

    it('lineStart(1) = 5 (after 4-byte emoji + 1-byte \\n)', () => {
        expect(buildByteLineMap(content).lineStart(1)).toBe(5);
    });

    it('lineOf(0) = 0: start of emoji', () => {
        expect(buildByteLineMap(content).lineOf(0)).toBe(0);
    });

    it('lineOf(3) = 0: last byte of emoji (4-byte char advances by 4)', () => {
        // The emoji occupies bytes 0-3. Byte 3 is the last byte of the emoji,
        // still on line 0 — boundaries are in byte space, not JS-char space.
        expect(buildByteLineMap(content).lineOf(3)).toBe(0);
    });

    it('lineOf(5) = 1: first byte after the newline', () => {
        expect(buildByteLineMap(content).lineOf(5)).toBe(1);
    });

    it('lineOf(6) = 1: second byte of line 1', () => {
        expect(buildByteLineMap(content).lineOf(6)).toBe(1);
    });

    it('JS string length differs from byte length — verifying byte-space', () => {
        // In JS: '🎉\nhi'.length === 5
        //   🎉 = 2 surrogate code units, \n = 1, h = 1, i = 1 → total 5 JS chars
        // In UTF-8: 4 (emoji) + 1 (\n) + 1 (h) + 1 (i) = 7 bytes
        const jsLen = content.length; // 5
        const byteLen = new TextEncoder().encode(content).length; // 7
        expect(jsLen).toBe(5);
        expect(byteLen).toBe(7);
        // The map uses byte positions, so lineStart(1) = 5, not jsLen-based
        expect(buildByteLineMap(content).lineStart(1)).toBe(5);
    });
});

// ── Edge cases ───────────────────────────────────────────────────────────────

describe('empty string', () => {
    it('lineCount is 1', () => {
        expect(buildByteLineMap('').lineCount).toBe(1);
    });

    it('lineStart(0) = 0', () => {
        expect(buildByteLineMap('').lineStart(0)).toBe(0);
    });

    it('lineOf(0) = 0', () => {
        expect(buildByteLineMap('').lineOf(0)).toBe(0);
    });
});

describe('single-line (no newline)', () => {
    const content = 'hello';

    it('lineCount is 1', () => {
        expect(buildByteLineMap(content).lineCount).toBe(1);
    });

    it('lineStart(0) = 0', () => {
        expect(buildByteLineMap(content).lineStart(0)).toBe(0);
    });

    it('lineOf(0) = 0', () => {
        expect(buildByteLineMap(content).lineOf(0)).toBe(0);
    });

    it('lineOf(4) = 0: last byte is still line 0', () => {
        expect(buildByteLineMap(content).lineOf(4)).toBe(0);
    });
});

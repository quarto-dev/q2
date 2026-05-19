/**
 * Unit tests for `formatRelativeTime`.
 *
 * The function lives at the seam where two providers with different
 * timestamp units feed the same renderer: `git blame` emits Unix
 * **seconds**, Automerge emits Unix **milliseconds**. The `< 1e12`
 * heuristic in the implementation is what reconciles them. These
 * tests pin that heuristic alongside the per-bucket wording so a
 * future "just simplify it to ms" refactor breaks loudly.
 *
 * The Rust CLI test in
 * `crates/quarto/tests/attribution_cli_e2e.rs` pins the same unit
 * contract on the producer side.
 *
 * @vitest-environment jsdom
 */

import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import { formatRelativeTime } from './attribution';

// Round, easy-to-reason-about reference point: 1_800_000_000_000 ms
// since the epoch corresponds to ~January 2027. Picking a value that
// far exceeds 1e12 means every ms-as-ms branch sees a positive diff
// without us having to think about wall-clock drift.
const NOW_MS = 1_800_000_000_000;
const NOW_SEC = NOW_MS / 1000;

describe('formatRelativeTime', () => {
    beforeAll(() => {
        vi.useFakeTimers();
        vi.setSystemTime(NOW_MS);
    });
    afterAll(() => {
        vi.useRealTimers();
    });

    describe('millisecond inputs (Automerge / hub-client wire)', () => {
        it('returns "just now" for sub-minute deltas', () => {
            expect(formatRelativeTime(NOW_MS)).toBe('just now');
            expect(formatRelativeTime(NOW_MS - 1_000)).toBe('just now');
            expect(formatRelativeTime(NOW_MS - 59_000)).toBe('just now');
        });

        it('crosses to "1m ago" at exactly 60 seconds', () => {
            expect(formatRelativeTime(NOW_MS - 60_000)).toBe('1m ago');
            expect(formatRelativeTime(NOW_MS - 90_000)).toBe('1m ago');
            expect(formatRelativeTime(NOW_MS - 59 * 60_000)).toBe('59m ago');
        });

        it('crosses to "1h ago" at exactly 60 minutes', () => {
            expect(formatRelativeTime(NOW_MS - 60 * 60_000)).toBe('1h ago');
            expect(formatRelativeTime(NOW_MS - 23 * 3_600_000)).toBe('23h ago');
        });

        it('crosses to "1d ago" at exactly 24 hours', () => {
            expect(formatRelativeTime(NOW_MS - 24 * 3_600_000)).toBe('1d ago');
            expect(formatRelativeTime(NOW_MS - 7 * 24 * 3_600_000)).toBe('7d ago');
        });
    });

    describe('second inputs (git blame provider)', () => {
        // Sub-1e12 values are interpreted as Unix seconds and
        // multiplied by 1000 inside the function. The same logical
        // "90 seconds ago" yields the same string regardless of unit,
        // which is the whole point of the heuristic.
        it('treats sub-1e12 values as Unix seconds (multiplied to ms)', () => {
            expect(formatRelativeTime(NOW_SEC - 90)).toBe('1m ago');
            expect(formatRelativeTime(NOW_SEC - 3600)).toBe('1h ago');
            expect(formatRelativeTime(NOW_SEC - 86_400)).toBe('1d ago');
        });
    });

    describe('1e12 unit-dispatch boundary', () => {
        // Threshold check is `timestamp < 1e12`, so 1e12 itself is
        // treated as already-milliseconds. 1e12 ms ≈ 2001-09-09, so
        // from NOW_MS (~2027) it sits ~9259 days in the past — far
        // enough to land in the "Nd ago" bucket regardless of small
        // drift in NOW_MS.
        it('treats exactly 1e12 as milliseconds', () => {
            expect(formatRelativeTime(1e12)).toMatch(/^\d+d ago$/);
        });

        // Values just below the threshold are multiplied. (1e12 - 1)
        // seconds × 1000 ms/sec lands far in the future relative to
        // NOW_MS, which yields a negative diff. `Math.floor` of a
        // large negative number is < 60, so the first branch wins.
        // We don't pin the exact wording — we pin that the seconds
        // and ms interpretations are *different* at the boundary.
        it('treats just-below-1e12 as seconds (different bucket from ms)', () => {
            const justBelow = formatRelativeTime(1e12 - 1);
            const justAt = formatRelativeTime(1e12);
            expect(justBelow).not.toBe(justAt);
        });
    });
});

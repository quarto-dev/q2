/**
 * P2.3b integration tests: tileForAnchorR0 + findReanchorCandidate
 *
 * TDD: these tests were written BEFORE implementation. They fail until
 * tileForAnchorR0 and findReanchorCandidate are exported from lockedTiles.ts.
 *
 * tileForAnchorR0(host, pool, anchorR0):
 *   Returns the visible locked-tile DOM element for a byte offset.
 *   Exact match preferred; nearest-at/after as fallback; null if nothing qualifies.
 *
 * findReanchorCandidate(pool, content, anchorR0, anchorSlice):
 *   Pure pool scan + content-verify for the P2.3b re-anchor candidate.
 *   Returns { r0, r1 } or null.
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { tileForAnchorR0, findReanchorCandidate } from './lockedTiles';

afterEach(() => {
    vi.restoreAllMocks();
    document.body.innerHTML = '';
});

/* ─── helpers ──────────────────────────────────────────────────────────────── */

function rect(
    left: number, top: number, right: number, bottom: number,
): DOMRect {
    return {
        left, top, right, bottom,
        x: left, y: top,
        width: right - left,
        height: bottom - top,
        toJSON: () => ({}),
    };
}

const VISIBLE = rect(0, 0, 200, 40);
const ZERO    = rect(0, 0, 0, 0);

function mockRect(el: Element, r: DOMRect) {
    vi.spyOn(el, 'getBoundingClientRect').mockReturnValue(r);
}

/**
 * Build a host div with tiles. `tiles` is an array of { poolId, r0 } so
 * we can set data-block-pool-id. `pool` is the parallel array where
 * pool[poolId] = { t: 0, r: [r0, r0+10], d: 0 }.
 */
function makeHost(
    tiles: Array<{ poolId: number; r0: number; visible?: boolean }>,
): { host: HTMLElement; elements: HTMLElement[]; pool: unknown[] } {
    const host = document.createElement('div');
    document.body.appendChild(host);

    const pool: unknown[] = [];
    const elements: HTMLElement[] = [];

    for (const { poolId, r0, visible = true } of tiles) {
        const el = document.createElement('p');
        el.setAttribute('data-block-pool-id', String(poolId));
        host.appendChild(el);
        mockRect(el, visible ? VISIBLE : ZERO);
        // extend pool array to accommodate poolId
        while (pool.length <= poolId) pool.push(null);
        pool[poolId] = { t: 0, r: [r0, r0 + 10], d: 0 };
        elements.push(el);
    }

    return { host, elements, pool };
}

/* ─── tileForAnchorR0 ───────────────────────────────────────────────────────── */

describe('tileForAnchorR0 — exact hit', () => {
    it('returns the tile whose pool r[0] === anchorR0 exactly', () => {
        const { host, elements, pool } = makeHost([
            { poolId: 0, r0: 100 },
            { poolId: 1, r0: 200 },
            { poolId: 2, r0: 300 },
        ]);
        const result = tileForAnchorR0(host, pool, 200);
        expect(result).toBe(elements[1]);
    });
});

describe('tileForAnchorR0 — nearest-at/after when exact missing', () => {
    it('returns the tile with smallest r[0] >= anchorR0 when exact not found', () => {
        const { host, elements, pool } = makeHost([
            { poolId: 0, r0: 100 },
            { poolId: 1, r0: 200 },
            { poolId: 2, r0: 300 },
        ]);
        // anchorR0=150 doesn't exist; nearest >= 150 is r0=200
        const result = tileForAnchorR0(host, pool, 150);
        expect(result).toBe(elements[1]);
    });

    it('returns the tile at the exact anchorR0 position when it happens to equal r0', () => {
        const { host, elements, pool } = makeHost([
            { poolId: 0, r0: 100 },
            { poolId: 1, r0: 200 },
        ]);
        const result = tileForAnchorR0(host, pool, 100);
        expect(result).toBe(elements[0]);
    });
});

describe('tileForAnchorR0 — null when nothing at/after', () => {
    it('returns null when all tiles have r[0] < anchorR0', () => {
        const { host, pool } = makeHost([
            { poolId: 0, r0: 100 },
            { poolId: 1, r0: 200 },
        ]);
        const result = tileForAnchorR0(host, pool, 999);
        expect(result).toBeNull();
    });

    it('returns null for an empty host', () => {
        const host = document.createElement('div');
        document.body.appendChild(host);
        const result = tileForAnchorR0(host, [], 0);
        expect(result).toBeNull();
    });
});

describe('tileForAnchorR0 — skips hidden tiles', () => {
    it('skips hidden (zero-rect) tiles; returns nearest visible at/after', () => {
        // tile0 r0=100 visible; tile1 r0=200 hidden; tile2 r0=300 visible
        // anchorR0=200: exact match is tile1 but it's hidden.
        // enumerateLockedTiles excludes hidden tiles, so only r0=100 and r0=300 are candidates.
        // nearest visible at/after 200 is r0=300 (tile2).
        const { host, elements, pool } = makeHost([
            { poolId: 0, r0: 100, visible: true },
            { poolId: 1, r0: 200, visible: false },  // hidden
            { poolId: 2, r0: 300, visible: true },
        ]);
        const result = tileForAnchorR0(host, pool, 200);
        expect(result).toBe(elements[2]);
    });

    it('returns null when all at/after tiles are hidden', () => {
        const { host, pool } = makeHost([
            { poolId: 0, r0: 100, visible: true },
            { poolId: 1, r0: 200, visible: false },
            { poolId: 2, r0: 300, visible: false },
        ]);
        const result = tileForAnchorR0(host, pool, 200);
        expect(result).toBeNull();
    });
});

/* ─── tileForAnchorR0 — exactOnly option ────────────────────────────────────── */

describe('tileForAnchorR0 — exactOnly: true', () => {
    it('returns the tile when it is visible at exactly anchorR0', () => {
        const { host, elements, pool } = makeHost([
            { poolId: 0, r0: 100, visible: true },
            { poolId: 1, r0: 200, visible: true },
            { poolId: 2, r0: 300, visible: true },
        ]);
        const result = tileForAnchorR0(host, pool, 200, { exactOnly: true });
        expect(result).toBe(elements[1]);
    });

    it('returns null when the tile at exactly anchorR0 is hidden (zero rect)', () => {
        // The re-anchored tile is hidden, but a later visible tile exists.
        // exactOnly must return null — NOT the later visible tile.
        const { host, pool } = makeHost([
            { poolId: 0, r0: 100, visible: true },
            { poolId: 1, r0: 200, visible: false },  // hidden — our re-anchored tile
            { poolId: 2, r0: 300, visible: true },   // a later visible tile exists
        ]);
        const result = tileForAnchorR0(host, pool, 200, { exactOnly: true });
        // Must be null — the re-anchored tile is hidden; must NOT fall back to r0=300
        expect(result).toBeNull();
    });

    it('returns null when no visible tile exists at exactly anchorR0 (no tile at that r0)', () => {
        const { host, pool } = makeHost([
            { poolId: 0, r0: 100, visible: true },
            { poolId: 1, r0: 300, visible: true },
        ]);
        // anchorR0=200 has no tile — exactOnly must not fall back to nearest
        const result = tileForAnchorR0(host, pool, 200, { exactOnly: true });
        expect(result).toBeNull();
    });

    it('default behavior (exactOnly absent) still returns nearest when exact is hidden', () => {
        // Confirm the DEFAULT path is unchanged — exactOnly does not affect it.
        const { host, elements, pool } = makeHost([
            { poolId: 0, r0: 100, visible: true },
            { poolId: 1, r0: 200, visible: false },
            { poolId: 2, r0: 300, visible: true },
        ]);
        // Default: exact hidden → skip; nearest visible at/after 200 is r0=300
        const result = tileForAnchorR0(host, pool, 200);
        expect(result).toBe(elements[2]);
    });
});

/* ─── hidden-drop correctness: multi-tile pool with later visible tile ───────── */

describe('tileForAnchorR0 — exactOnly hidden-drop correctness (multi-tile pool)', () => {
    it('hidden-drop is correctly detected when re-anchored tile is hidden but a later visible tile exists', () => {
        // This is the Fix 1 scenario: after re-anchor to r0=200, that tile is hidden.
        // But a visible tile exists at r0=300. The hidden-drop check MUST use exactOnly:
        //   - old code: tileForAnchorR0(host, pool, 200) → returns r0=300 (non-null) → hidden-drop MISSED
        //   - new code: tileForAnchorR0(host, pool, 200, {exactOnly:true}) → null → hidden-drop DETECTED
        const { host, pool } = makeHost([
            { poolId: 0, r0: 100, visible: true },
            { poolId: 1, r0: 200, visible: false },  // re-anchored tile — hidden
            { poolId: 2, r0: 300, visible: true },   // later visible tile (should not rescue)
        ]);

        // Old (broken) behavior: non-null because r0=300 is visible
        const oldResult = tileForAnchorR0(host, pool, 200);
        expect(oldResult).not.toBeNull(); // confirms the bug exists in the default path

        // New (correct) behavior: null because the exact tile at r0=200 is hidden
        const newResult = tileForAnchorR0(host, pool, 200, { exactOnly: true });
        expect(newResult).toBeNull();
    });
});

/* ─── findReanchorCandidate ─────────────────────────────────────────────────── */

// Simple content: 'hello\n' (6 bytes, 0..6) + 'world\n' (6 bytes, 6..12)
const SIMPLE_CONTENT = 'hello\nworld\n';

describe('findReanchorCandidate — exact match re-anchors', () => {
    it('returns { r0, r1 } when exact candidate content matches anchorSlice', () => {
        const pool: unknown[] = [
            { t: 0, r: [0, 6], d: 0 },    // 'hello\n' → trimmed 'hello'
            { t: 0, r: [6, 12], d: 0 },   // 'world\n' → trimmed 'world'
        ];
        const result = findReanchorCandidate(pool, SIMPLE_CONTENT, 0, 'hello');
        expect(result).toEqual({ r0: 0, r1: 6 });
    });

    it('returns null when exact candidate content does NOT match anchorSlice (block edited under you)', () => {
        const pool: unknown[] = [
            { t: 0, r: [0, 6], d: 0 },
            { t: 0, r: [6, 12], d: 0 },
        ];
        const result = findReanchorCandidate(pool, SIMPLE_CONTENT, 0, 'CHANGED');
        expect(result).toBeNull();
    });
});

describe('findReanchorCandidate — re-anchor nearest when anchorR0 absent from pool', () => {
    it('re-anchors when anchorR0 not in new pool but nearest content matches (shift scenario)', () => {
        // anchorR0=100 (was our block's location). After a 10-byte insert above,
        // our block is now at r0=110. Pool has no entry at r[0]=100.
        // nearest at/after 100 is r0=110. Content at [110..116] is 'hello\n' → 'hello'.
        const bigContent = 'X'.repeat(110) + 'hello\n' + 'Y'.repeat(20);
        const pool: unknown[] = [
            { t: 0, r: [0, 110], d: 0 },    // filler block
            { t: 0, r: [110, 116], d: 0 },  // 'hello\n' → our block (was at r0=100)
            { t: 0, r: [116, 136], d: 0 },  // another block after
        ];
        const result = findReanchorCandidate(pool, bigContent, 100, 'hello');
        expect(result).toEqual({ r0: 110, r1: 116 });
    });

    it('exact at anchorR0 fails content check → null (no fallback to nearest)', () => {
        // anchorR0=0 has an exact entry but its content is 'world', not 'hello'.
        // A block at r0=6 has content 'hello' but nearest is not tried after exact fails.
        const pool: unknown[] = [
            { t: 0, r: [0, 6], d: 0 },    // 'hello\n' BUT we test with anchorSlice='DIFFERENT'
            { t: 0, r: [6, 12], d: 0 },   // 'world\n'
        ];
        // anchorSlice='DIFFERENT', exact at r0=0 has content 'hello' ≠ 'DIFFERENT' → null
        const result = findReanchorCandidate(pool, SIMPLE_CONTENT, 0, 'DIFFERENT');
        expect(result).toBeNull();
    });
});

describe('findReanchorCandidate — no candidate at/after → null', () => {
    it('returns null when no pool entry has r[0] >= anchorR0', () => {
        const pool: unknown[] = [
            { t: 0, r: [0, 6], d: 0 },
            { t: 0, r: [6, 12], d: 0 },
        ];
        const result = findReanchorCandidate(pool, SIMPLE_CONTENT, 999, 'hello');
        expect(result).toBeNull();
    });

    it('returns null for empty pool', () => {
        const result = findReanchorCandidate([], SIMPLE_CONTENT, 0, 'hello');
        expect(result).toBeNull();
    });
});

describe('findReanchorCandidate — non-original entries ignored', () => {
    it('ignores pool entries with t !== 0', () => {
        const pool: unknown[] = [
            { t: 1, r: [0, 6], d: 0 },   // t=1, not original → ignored
            { t: 0, r: [6, 12], d: 0 },  // t=0, original → candidate
        ];
        // anchorR0=0 → no t=0 entry at r[0]=0; nearest at/after 0: t=0 entry at r0=6 → 'world'
        const result = findReanchorCandidate(pool, SIMPLE_CONTENT, 0, 'world');
        expect(result).toEqual({ r0: 6, r1: 12 });
    });

    it('ignores pool entries with d !== 0', () => {
        const pool: unknown[] = [
            { t: 0, r: [0, 6], d: 1 },   // d=1, included file → ignored
            { t: 0, r: [6, 12], d: 0 },  // d=0, active → candidate
        ];
        const result = findReanchorCandidate(pool, SIMPLE_CONTENT, 0, 'world');
        expect(result).toEqual({ r0: 6, r1: 12 });
    });
});

describe('findReanchorCandidate — CRLF normalization', () => {
    it('handles CRLF in content correctly via normalizeLineEndings', () => {
        // content with CRLF between blocks
        const crlfContent = 'hello\r\nworld\r\n';
        // 'hello\r\n' = 7 bytes (0..7), 'world\r\n' = 7 bytes (7..14)
        const pool: unknown[] = [
            { t: 0, r: [0, 7], d: 0 },    // 'hello\r\n' → normalized 'hello\n' → trimmed 'hello'
            { t: 0, r: [7, 14], d: 0 },   // 'world\r\n' → normalized 'world\n' → trimmed 'world'
        ];
        // anchorSlice was captured as normalizeLineEndings(slice).trimEnd() = 'hello'
        const result = findReanchorCandidate(pool, crlfContent, 0, 'hello');
        expect(result).toEqual({ r0: 0, r1: 7 });
    });
});

describe('findReanchorCandidate — nearest is smallest r[0] at/after, not first in array', () => {
    it('returns null when no at/after candidate content matches (all are different blocks)', () => {
        // Pool with entries at/after anchorR0, but none have content matching anchorSlice.
        // content: 150 X's + 'AAAAAAA' (7 bytes) + 'BBBBBBB' (7 bytes)
        // anchorR0=150: entries at r0=150 ('AAAAAAA') and r0=157 ('BBBBBBB')
        // Neither matches anchorSlice='hello'
        const content = 'X'.repeat(150) + 'AAAAAAA' + 'BBBBBBB';
        const pool: unknown[] = [
            { t: 0, r: [157, 164], d: 0 },  // further, content 'BBBBBBB' — not 'hello'
            { t: 0, r: [150, 157], d: 0 },  // nearest at/after 150, content 'AAAAAAA' — not 'hello'
            { t: 0, r: [0, 150], d: 0 },    // before anchorR0=150 → excluded from at/after
        ];
        // No candidate content matches 'hello' → null
        const result = findReanchorCandidate(pool, content, 150, 'hello');
        expect(result).toBeNull();
    });

    it('finds correct nearest among multiple candidates when content matches', () => {
        // anchorR0=100 absent. Nearest at/after 100: r0=110 with 'hello'
        const content = 'A'.repeat(110) + 'hello\n' + 'B'.repeat(90) + 'C'.repeat(20);
        const pool: unknown[] = [
            { t: 0, r: [200, 206], d: 0 },  // r0=200, content 'CCCCCC' — further
            { t: 0, r: [110, 116], d: 0 },  // r0=110, content 'hello\n' — nearest and matches
            { t: 0, r: [0, 110], d: 0 },    // r0=0, before anchorR0=100 → excluded
        ];
        const result = findReanchorCandidate(pool, content, 100, 'hello');
        expect(result).toEqual({ r0: 110, r1: 116 });
    });
});

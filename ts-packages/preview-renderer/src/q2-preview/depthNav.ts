/**
 * depthNav.ts — pure depth-cursor navigation utilities (Plan P3.3).
 *
 * All exports are pure functions (no DOM, no React). The one platform sniff
 * (`detectPlatform`) accepts an injectable navigator-like object so it can be
 * tested without touching `globalThis.navigator`.
 */

// ── Types ─────────────────────────────────────────────────────────────────────

export interface DepthSurface {
  r0: number;
  r1: number;
}

// ── parseSiKey ────────────────────────────────────────────────────────────────

/**
 * Parse a source-index key `"<t>:<r0>-<r1>:<d>"` into its parts.
 * Returns null if the key is malformed.
 *
 * Examples:
 *   "0:42-87:0" → { t: 0, r0: 42, r1: 87, d: 0 }
 *   "garbage"   → null
 */
export function parseSiKey(
  key: string,
): { t: number; r0: number; r1: number; d: number } | null {
  // Format: <t>:<r0>-<r1>:<d>
  const match = /^(\d+):(\d+)-(\d+):(\d+)$/.exec(key);
  if (!match) return null;
  return {
    t: Number(match[1]),
    r0: Number(match[2]),
    r1: Number(match[3]),
    d: Number(match[4]),
  };
}

// ── buildDepthSurfaces ────────────────────────────────────────────────────────

/**
 * Extract the depth-navigable block surfaces from a source index.
 *
 * - Skips entries whose reachabilityClass === 'Opaque'.
 * - Skips entries whose key fails to parse.
 * - Returns DepthSurface[] sorted by r0 ascending, then r1 DESCENDING
 *   (outer before inner at same r0).
 * - Dedupes surfaces with identical (r0, r1).
 * - null/undefined input → [].
 */
export function buildDepthSurfaces(
  sourceIndex: Map<string, { reachabilityClass: string }> | null | undefined,
): DepthSurface[] {
  if (!sourceIndex) return [];

  const seen = new Set<string>();
  const surfaces: DepthSurface[] = [];

  for (const [key, entry] of sourceIndex) {
    if (entry.reachabilityClass === 'Opaque') continue;
    const parsed = parseSiKey(key);
    if (!parsed) continue;
    const dedupeKey = `${parsed.r0}:${parsed.r1}`;
    if (seen.has(dedupeKey)) continue;
    seen.add(dedupeKey);
    surfaces.push({ r0: parsed.r0, r1: parsed.r1 });
  }

  // Sort: r0 ascending, then r1 descending (outer before inner)
  surfaces.sort((a, b) => {
    if (a.r0 !== b.r0) return a.r0 - b.r0;
    return b.r1 - a.r1;
  });

  return surfaces;
}

// ── parentSurface ─────────────────────────────────────────────────────────────

/**
 * The immediate PARENT surface of the cursor: the tightest surface that
 * strictly contains [cursorR0, cursorR1).
 *
 * "strictly contains": s.r0 <= cursorR0 && s.r1 >= cursorR1 &&
 *   NOT (s.r0 === cursorR0 && s.r1 === cursorR1).
 *
 * Among all strict containers, returns the one with the LARGEST r0
 * (tiebreak: SMALLEST r1) — the tightest.
 *
 * Returns null if the cursor has no container (already outermost).
 */
export function parentSurface(
  surfaces: DepthSurface[],
  cursorR0: number,
  cursorR1: number,
): DepthSurface | null {
  let best: DepthSurface | null = null;

  for (const s of surfaces) {
    const strictlyContains =
      s.r0 <= cursorR0 &&
      s.r1 >= cursorR1 &&
      !(s.r0 === cursorR0 && s.r1 === cursorR1);
    if (!strictlyContains) continue;

    if (best === null) {
      best = s;
    } else {
      // Prefer largest r0 (tightest container); tiebreak: smallest r1
      if (s.r0 > best.r0 || (s.r0 === best.r0 && s.r1 < best.r1)) {
        best = s;
      }
    }
  }

  return best;
}

// ── childSurfaceToward ────────────────────────────────────────────────────────

/**
 * The direct CHILD surface to descend into, heading toward leafAnchorR0.
 *
 * Algorithm:
 * 1. `contained` = surfaces strictly contained by the cursor.
 * 2. Prefer the child on the path to the leaf: among `contained`,
 *    those with s.r0 <= leafAnchorR0 < s.r1; return the OUTERMOST
 *    (smallest r0, tiebreak largest r1) that is a direct child of the cursor.
 *    Actually: return the outermost contained surface whose range contains
 *    leafAnchorR0. This is the direct child toward the leaf.
 * 3. Fallback (leaf deleted/gone): among `contained`, find the FIRST direct
 *    children (those not strictly contained by any other `contained` surface),
 *    and return the one with smallest r0 (tiebreak largest r1).
 * 4. Returns null if `contained` is empty (cursor is a leaf).
 */
export function childSurfaceToward(
  surfaces: DepthSurface[],
  cursorR0: number,
  cursorR1: number,
  leafAnchorR0: number,
): DepthSurface | null {
  // Step 1: all surfaces strictly contained by the cursor
  const contained = surfaces.filter(
    s =>
      s.r0 >= cursorR0 &&
      s.r1 <= cursorR1 &&
      !(s.r0 === cursorR0 && s.r1 === cursorR1),
  );

  if (contained.length === 0) return null;

  // Step 2: on the path to the leaf — contained surfaces that contain leafAnchorR0
  const onPath = contained.filter(
    s => s.r0 <= leafAnchorR0 && leafAnchorR0 < s.r1,
  );

  if (onPath.length > 0) {
    // Return the outermost (smallest r0, tiebreak largest r1)
    return onPath.reduce((best, s) => {
      if (s.r0 < best.r0) return s;
      if (s.r0 === best.r0 && s.r1 > best.r1) return s;
      return best;
    });
  }

  // Step 3: fallback — first direct children of the cursor
  // A "direct child" is a contained surface not strictly contained by any
  // other contained surface.
  const directChildren = contained.filter(
    s => !contained.some(
      other =>
        other !== s &&
        other.r0 <= s.r0 &&
        other.r1 >= s.r1 &&
        !(other.r0 === s.r0 && other.r1 === s.r1),
    ),
  );

  if (directChildren.length === 0) return null;

  // Return first direct child: smallest r0, tiebreak largest r1
  return directChildren.reduce((best, s) => {
    if (s.r0 < best.r0) return s;
    if (s.r0 === best.r0 && s.r1 > best.r1) return s;
    return best;
  });
}

// ── classifyDepthKey ──────────────────────────────────────────────────────────

/**
 * Classify a keyboard chord as a depth move, per platform.
 *
 * mac:   metaKey && ctrlKey && !altKey && !shiftKey
 * other: altKey && shiftKey && !metaKey && !ctrlKey
 *
 * With the platform's modifier set held:
 *   'ArrowLeft'  → 'out'
 *   'ArrowRight' → 'in'
 *   anything else → null
 *
 * Any non-matching modifier combo → null (never swallow bare arrows).
 */
export function classifyDepthKey(
  e: {
    key: string;
    metaKey: boolean;
    ctrlKey: boolean;
    altKey: boolean;
    shiftKey: boolean;
  },
  platform: 'mac' | 'other',
): 'in' | 'out' | null {
  const modMatch =
    platform === 'mac'
      ? e.metaKey && e.ctrlKey && !e.altKey && !e.shiftKey
      : e.altKey && e.shiftKey && !e.metaKey && !e.ctrlKey;

  if (!modMatch) return null;

  if (e.key === 'ArrowLeft') return 'out';
  if (e.key === 'ArrowRight') return 'in';
  return null;
}

// ── detectPlatform ────────────────────────────────────────────────────────────

/**
 * Detect the platform from a navigator-like object (injectable for tests).
 * Returns 'mac' if platform/userAgent matches /mac/i, else 'other'.
 * Defaults to reading the global `navigator` when no arg is given.
 */
export function detectPlatform(
  nav?: { platform?: string; userAgent?: string },
): 'mac' | 'other' {
  const n = nav ?? (typeof navigator !== 'undefined' ? navigator : {});
  const haystack = `${n.platform ?? ''} ${n.userAgent ?? ''}`;
  return /mac/i.test(haystack) ? 'mac' : 'other';
}

// ── buildDepthCommitDestination ───────────────────────────────────────────────

/**
 * Build the commit-destination JSON for the depth ("self-heal on write")
 * commit path, from the LIVE edit target.
 *
 * Returns null when there is no active target (commit no-ops).
 * Shape: JSON.stringify({ t: 0, r: [anchorR0, anchorR1], d: 0 })
 */
export function buildDepthCommitDestination(
  et: { anchorR0: number; anchorR1: number } | null | undefined,
): string | null {
  if (!et) return null;
  return JSON.stringify({ t: 0, r: [et.anchorR0, et.anchorR1], d: 0 });
}

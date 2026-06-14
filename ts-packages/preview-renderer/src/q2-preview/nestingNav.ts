/**
 * nestingNav.ts — pure nesting-cursor navigation utilities (Plan P3.3).
 *
 * All exports are pure functions (no DOM, no React). The one platform sniff
 * (`detectPlatform`) accepts an injectable navigator-like object so it can be
 * tested without touching `globalThis.navigator`.
 */

import type { BlockNode } from '../framework/types';
import type { SourceIndexEntry } from './sourceIndex';

// ── Types ─────────────────────────────────────────────────────────────────────

export interface NestingSurface {
  r0: number;
  r1: number;
}

export interface AncestorCrumb {
  label: string;
  r0: number;
  r1: number;
  isCurrent: boolean;
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

// ── buildNestingSurfaces ────────────────────────────────────────────────────────

/**
 * Extract the nesting-navigable block surfaces from a source index.
 *
 * - Skips entries whose reachabilityClass === 'Opaque'.
 * - Skips entries whose key fails to parse.
 * - Returns NestingSurface[] sorted by r0 ascending, then r1 DESCENDING
 *   (outer before inner at same r0).
 * - Dedupes surfaces with identical (r0, r1).
 * - null/undefined input → [].
 */
export function buildNestingSurfaces(
  sourceIndex: Map<string, { reachabilityClass: string }> | null | undefined,
): NestingSurface[] {
  if (!sourceIndex) return [];

  const seen = new Set<string>();
  const surfaces: NestingSurface[] = [];

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
  surfaces: NestingSurface[],
  cursorR0: number,
  cursorR1: number,
): NestingSurface | null {
  let best: NestingSurface | null = null;

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

// ── topBlockR0 ────────────────────────────────────────────────────────────────

/**
 * The `r0` of the OUTERMOST nesting surface containing `[r0, r1]` — i.e. climb
 * `parentSurface` repeatedly until there is no container, returning the last
 * non-null surface's `r0` (or the cursor's own `r0` if it is already outermost).
 *
 * Used by §1's geometry snapshot to derive the top-level block's start byte so
 * snapshot keys can be stored block-relative (`r0 − topBlockR0`), making them
 * shift-invariant under an insert-above. Computed from the same source index at
 * both capture and lookup. Pure (no DOM).
 *
 * Note: returns the last non-null surface's r0 — NOT null — so an already-top
 * cursor maps to its own r0.
 */
export function topBlockR0(
  surfaces: NestingSurface[],
  r0: number,
  r1: number,
): number {
  let cur: NestingSurface = { r0, r1 };
  for (;;) {
    const p = parentSurface(surfaces, cur.r0, cur.r1);
    if (!p) return cur.r0;
    cur = p;
  }
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
  surfaces: NestingSurface[],
  cursorR0: number,
  cursorR1: number,
  leafAnchorR0: number,
): NestingSurface | null {
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

// ── classifyNestingKey ──────────────────────────────────────────────────────────

/**
 * Classify a keyboard chord as a nesting move, per platform.
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
export function classifyNestingKey(
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

// ── labelForSourceNode ────────────────────────────────────────────────────────

/**
 * Return a human-readable label for a Pandoc block node, optionally decorated
 * with the first identifying token from its Attr.
 *
 * Attr location by type:
 *   Div / CodeBlock / Figure / Table → c[0]
 *   Header                            → c[1]
 *   All others (Para, Plain, BlockQuote, BulletList, …) → no Attr
 *
 * Priority: id > first class > bare type.
 * Defensive: returns "" when node is null/undefined or has no string `t`.
 */
export function labelForSourceNode(node: BlockNode): string {
  if (!node || typeof node.t !== 'string') return '';
  const t = node.t;
  const c = (node as unknown as { c?: unknown }).c;

  // Extract the Attr based on block type
  let attr: unknown;
  if (t === 'Div' || t === 'CodeBlock' || t === 'Figure' || t === 'Table') {
    attr = Array.isArray(c) ? (c as unknown[])[0] : undefined;
  } else if (t === 'Header') {
    attr = Array.isArray(c) ? (c as unknown[])[1] : undefined;
  }

  // Defensive: Attr must be array of length >= 2, attr[0] is string (id), attr[1] is array (classes)
  if (
    Array.isArray(attr) &&
    attr.length >= 2 &&
    typeof attr[0] === 'string' &&
    Array.isArray(attr[1])
  ) {
    const id = attr[0] as string;
    const classes = attr[1] as string[];
    if (id.length > 0) return `${t}#${id}`;
    if (classes.length > 0) return `${t}.${classes[0]}`;
  }

  return t;
}

// ── buildAncestorPath ─────────────────────────────────────────────────────────

/**
 * Build the ancestor path for the breadcrumb chip.
 *
 * For each (non-Opaque) source index entry whose range contains the cursor
 * position, produces an AncestorCrumb. Dedupes by (r0,r1). Sorts
 * outermost → innermost. Marks the crumb whose range exactly matches the
 * cursor as `isCurrent: true` (always the last entry).
 *
 * Returns [] for null/undefined sourceIndex.
 */
export function buildAncestorPath(
  sourceIndex: Map<string, SourceIndexEntry> | null | undefined,
  cursorR0: number,
  cursorR1: number,
): AncestorCrumb[] {
  if (!sourceIndex) return [];

  const seen = new Map<string, { r0: number; r1: number; sourceNode: BlockNode }>();

  for (const [key, entry] of sourceIndex) {
    if (entry.reachabilityClass === 'Opaque') continue;
    const parsed = parseSiKey(key);
    if (!parsed) continue;
    const { r0, r1 } = parsed;
    // Keep only surfaces that contain the cursor
    if (r0 > cursorR0 || r1 < cursorR1) continue;
    // Dedupe by (r0, r1) — first wins
    const dedupeKey = `${r0}:${r1}`;
    if (seen.has(dedupeKey)) continue;
    seen.set(dedupeKey, { r0, r1, sourceNode: entry.sourceNode });
  }

  // Sort outermost → innermost: r0 ascending, then r1 descending
  const items = Array.from(seen.values()).sort((a, b) => {
    if (a.r0 !== b.r0) return a.r0 - b.r0;
    return b.r1 - a.r1;
  });

  return items.map(({ r0, r1, sourceNode }) => ({
    label: labelForSourceNode(sourceNode),
    r0,
    r1,
    isCurrent: r0 === cursorR0 && r1 === cursorR1,
  }));
}

// ── buildNestingCommitDestination ───────────────────────────────────────────────

/**
 * Build the commit-destination JSON for the nesting cursor ("self-heal on write")
 * commit path, from the LIVE edit target.
 *
 * Returns null when there is no active target (commit no-ops).
 * Shape: JSON.stringify({ t: 0, r: [anchorR0, anchorR1], d: 0 })
 */
export function buildNestingCommitDestination(
  et: { anchorR0: number; anchorR1: number } | null | undefined,
): string | null {
  if (!et) return null;
  return JSON.stringify({ t: 0, r: [et.anchorR0, et.anchorR1], d: 0 });
}

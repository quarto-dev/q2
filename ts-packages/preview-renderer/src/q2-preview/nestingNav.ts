/**
 * nestingNav.ts — pure nesting-cursor navigation utilities (Plan P3.3).
 *
 * All exports are pure functions (no DOM, no React). The one platform sniff
 * (`detectPlatform`) accepts an injectable navigator-like object so it can be
 * tested without touching `globalThis.navigator`.
 */

import type { BlockNode } from '../framework/types';
import type { SourceIndexEntry } from './sourceIndex';
import type { ByteLineMap } from '../utils/byteLineMap';
import { sliceUtf8 } from '../utils/utf8Slice';

// ── Types ─────────────────────────────────────────────────────────────────────

export interface NestingSurface {
  r0: number;
  r1: number;
}

export type CrumbCategory = 'container' | 'list' | 'quote' | 'leaf-text' | 'embed';

export interface AncestorCrumb {
  label: string;
  abbrev: string;
  category: CrumbCategory;
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

// ── depthOfSurface / relocateSurface (§2 commit-stable relocation) ──────────────

/**
 * The containment depth of `[r0,r1]` — the number of OTHER surfaces that strictly
 * contain it. Commit-stable: an in-place list rewrite renumbers / respaces but
 * does not change the nesting structure. Used to disambiguate two surfaces that
 * share a start line (a container and its first child) when relocating a
 * committed surface in a rewritten source index. Pure (no DOM).
 */
export function depthOfSurface(surfaces: NestingSurface[], r0: number, r1: number): number {
  let depth = 0;
  for (const s of surfaces) {
    if (s.r0 <= r0 && s.r1 >= r1 && !(s.r0 === r0 && s.r1 === r1)) depth++;
  }
  return depth;
}

/**
 * Relocate a committed surface in a (possibly rewritten) source index by its
 * commit-stable `(startLine, depth)`. Returns the first surface whose start line
 * is `startLine` AND whose containment depth is `depth`, or null when none match.
 * The depth filter is what disambiguates a container from its first child when
 * the two share a start line (Reflection: dirty crumb-jump relocation ambiguity).
 * Pure (no DOM); `map` is any byte→line lookup.
 */
export function relocateSurface(
  surfaces: NestingSurface[],
  map: { lineOf(b: number): number },
  startLine: number,
  depth: number,
): NestingSurface | null {
  for (const s of surfaces) {
    if (map.lineOf(s.r0) !== startLine) continue;
    if (depthOfSurface(surfaces, s.r0, s.r1) === depth) return s;
  }
  return null;
}

// ── surfaceAtLine ─────────────────────────────────────────────────────────────

/**
 * §1 C1/A2: Find the innermost navigable surface at source line L.
 *
 * Algorithm:
 *   1. Among all surfaces in `set` whose TRIMMED line-span (`surfaceLineSpan`)
 *      contains `L`, find the one with the GREATEST containment depth
 *      (`depthOfSurface`). This is the innermost surface that "covers" L.
 *   2. Amendment A2 — container-gap check: if that deepest surface is a
 *      CONTAINER (it has at least one other surface in `set` strictly contained
 *      within it) AND line L lies in NONE of those contained surfaces' trimmed
 *      spans, then L is a container-gap line (inter-item marker, empty item,
 *      blank-in-container, `<dt>` term). Return null so the caller can skip it.
 *   3. Only a true LEAF at L (no descendant surface contains L) is a valid
 *      landing. Return null when no surface's trimmed span contains L at all.
 *
 * Pure (no DOM). `content` and `map` are the document source and its byte→line
 * map, used by `surfaceLineSpan` for trimmed-span computation.
 */
export function surfaceAtLine(
  set: NestingSurface[],
  L: number,
  content: string,
  map: ByteLineMap,
): NestingSurface | null {
  // Local helper: true when `other` is strictly contained within `parent`
  // (same as "other is a descendant of parent" in the nesting hierarchy).
  const isStrictlyContainedBy = (other: NestingSurface, parent: NestingSurface): boolean =>
    other.r0 >= parent.r0 &&
    other.r1 <= parent.r1 &&
    !(other.r0 === parent.r0 && other.r1 === parent.r1);

  // Step 1: collect all surfaces whose trimmed span contains L.
  interface Candidate { s: NestingSurface; depth: number }
  const candidates: Candidate[] = [];
  for (const s of set) {
    const [s0, s1] = surfaceLineSpan(s, content, map);
    if (s0 <= L && L <= s1) {
      candidates.push({ s, depth: depthOfSurface(set, s.r0, s.r1) });
    }
  }

  if (candidates.length === 0) return null;

  // Pick the deepest (greatest depth); tiebreak by smallest r0 (determinism).
  candidates.sort((a, b) => {
    if (b.depth !== a.depth) return b.depth - a.depth; // descending depth
    return a.s.r0 - b.s.r0;
  });
  const deepest = candidates[0].s;

  // Step 2: A2 container-gap check.
  // The deepest surface is a CONTAINER if any other surface in `set` is
  // strictly contained within it.
  const hasDescendants = set.some(
    (other) => other !== deepest && isStrictlyContainedBy(other, deepest),
  );

  if (hasDescendants) {
    // Check whether L lies in ANY of the strictly-contained surfaces' spans.
    const liesInDescendant = set.some((other) => {
      if (other === deepest || !isStrictlyContainedBy(other, deepest)) return false;
      const [d0, d1] = surfaceLineSpan(other, content, map);
      return d0 <= L && L <= d1;
    });
    if (!liesInDescendant) return null; // container-gap line
  }

  return deepest;
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

// ── surfaceLineSpan ─────────────────────────────────────────────────────────────

/**
 * The 0-based [startLine, endLine] source-line span of a surface, computed from
 * its **trimmed** content range (Reflection #17).
 *
 * Node source ranges absorb the previous line's trailing whitespace at `r0` and
 * the next line's leading indent at `r1`, so RAW line spans of siblings overlap
 * (verified on the 3-level acceptance fixture: the sub-sub-item list `[39,60]`
 * has raw end byte 60 on the `nother` line). Trimming both ends collapses each
 * surface to the lines its visible content actually occupies, which is what
 * `childSurfaceTowardLine` needs to disambiguate siblings.
 *
 * **Byte hazard:** `r0`/`r1` are UTF-8 byte offsets, so the surface text is
 * extracted with `sliceUtf8` (NOT `String.prototype.slice`, which is UTF-16).
 * Leading/trailing whitespace is ASCII (space/tab/newline → 1 byte each), so the
 * trimmed-length deltas are byte counts and can be added to the byte offsets
 * directly. Pure (no DOM).
 */
export function surfaceLineSpan(
  surface: NestingSurface,
  content: string,
  map: ByteLineMap,
): [number, number] {
  const startLine = map.lineOf(surface.r0);
  const endLine = map.lineOf(Math.max(surface.r0, surface.r1 - 1));

  // Span the lines that carry the surface's *visible content*. A content
  // character is anything that is neither whitespace NOR a blockquote marker
  // (`>`). This generalises the old `trimStart`/`trimEnd` (which stripped only
  // whitespace): a node range absorbs the next line's leading indent / blockquote
  // prefix at `r1`, and inside a blockquote that prefix is `> ` — whose `>` a
  // whitespace trim cannot strip, so the span bled one line past its content and
  // overlapped the next item's content line (the loose-list-in-blockquote
  // down-nav "caught" bug). Outside a blockquote there are no `>` markers, so this
  // is byte-for-byte the prior behaviour. One pass over the surface text,
  // counting newlines to track the line.
  let line = startLine;
  let first = -1;
  let last = -1;
  for (const ch of sliceUtf8(content, surface.r0, surface.r1)) {
    if (ch === '\n') { line++; continue; }
    if (!/\s/.test(ch) && ch !== '>') {
      if (first === -1) first = line;
      last = line;
    }
  }

  // Degenerate all-blank surface (only whitespace / `>`): fall back to the raw
  // line span, matching the prior degenerate guard.
  return first === -1 ? [startLine, endLine] : [first, last];
}

// ── childSurfaceTowardLine ──────────────────────────────────────────────────────

/**
 * The direct CHILD surface to descend into, heading toward the source LINE the
 * caret is on (`Ls`). The line-space sibling of `childSurfaceToward` (byte
 * space): line space sidesteps the `> `/indent prefix (which only shifts
 * columns) and the trailing-whitespace overlap of raw byte ranges.
 *
 * Picks among the cursor's DIRECT children (same direct-child definition as
 * `childSurfaceToward`) the one whose **trimmed** line span (`surfaceLineSpan`)
 * contains `Ls`. Tiebreak when more than one contains `Ls`:
 *   1. start line == `Ls`  (the child that begins on the caret line)
 *   2. narrowest span
 *   3. nearest start line to `Ls`
 *   4. smallest `r0` (determinism)
 * When NO direct child contains `Ls` (e.g. caret beyond every child), returns
 * the nearest direct child by line distance to its span interval.
 *
 * Returns null when the cursor has no contained children (it is a leaf). The
 * caller falls back to the byte-space `childSurfaceToward` when there is no
 * readable caret at all. Pure (no DOM).
 */
export function childSurfaceTowardLine(
  surfaces: NestingSurface[],
  cursorR0: number,
  cursorR1: number,
  Ls: number,
  map: ByteLineMap,
  content: string,
): NestingSurface | null {
  const contained = surfaces.filter(
    s =>
      s.r0 >= cursorR0 &&
      s.r1 <= cursorR1 &&
      !(s.r0 === cursorR0 && s.r1 === cursorR1),
  );
  if (contained.length === 0) return null;

  // Direct children: contained surfaces not strictly contained by another
  // contained surface (mirrors childSurfaceToward step 3).
  const directChildren = contained.filter(
    s =>
      !contained.some(
        other =>
          other !== s &&
          other.r0 <= s.r0 &&
          other.r1 >= s.r1 &&
          !(other.r0 === s.r0 && other.r1 === s.r1),
      ),
  );
  if (directChildren.length === 0) return null;

  const withSpan = directChildren.map(s => ({ s, span: surfaceLineSpan(s, content, map) }));
  const containing = withSpan.filter(({ span }) => span[0] <= Ls && Ls <= span[1]);

  if (containing.length > 0) {
    return containing
      .slice()
      .sort((a, b) => {
        // 1. start line == Ls first
        const aStart = a.span[0] === Ls ? 0 : 1;
        const bStart = b.span[0] === Ls ? 0 : 1;
        if (aStart !== bStart) return aStart - bStart;
        // 2. narrowest span
        const aw = a.span[1] - a.span[0];
        const bw = b.span[1] - b.span[0];
        if (aw !== bw) return aw - bw;
        // 3. nearest start line to Ls
        const ad = Math.abs(a.span[0] - Ls);
        const bd = Math.abs(b.span[0] - Ls);
        if (ad !== bd) return ad - bd;
        // 4. determinism
        return a.s.r0 - b.s.r0;
      })[0].s;
  }

  // None contain Ls → nearest direct child by line distance to its span.
  const dist = (span: [number, number]) =>
    Ls < span[0] ? span[0] - Ls : Ls > span[1] ? Ls - span[1] : 0;
  return withSpan
    .slice()
    .sort((a, b) => {
      const da = dist(a.span);
      const db = dist(b.span);
      if (da !== db) return da - db;
      return a.s.r0 - b.s.r0;
    })[0].s;
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

// ── abbrevForSourceNode ───────────────────────────────────────────────────────

/**
 * Return a short abbreviation string for a source node, used as a compact
 * label in breadcrumb chips.
 *
 * Mirrors `labelForSourceNode`'s defensive coding style: safely reads
 * `node.t` as a string and `node.c` as an array, guarding with
 * `Array.isArray` and typeof checks before dereferencing.
 */
export function abbrevForSourceNode(node: BlockNode): string {
  if (!node || typeof node.t !== 'string') return '';
  const t = node.t;

  switch (t) {
    case 'Header': {
      // Header level is c[0]; Attr is c[1] (same layout as labelForSourceNode)
      const c = (node as unknown as { c?: unknown }).c;
      const level = Array.isArray(c) ? (c as unknown[])[0] : undefined;
      if (typeof level === 'number') return `H${level}`;
      return 'H';
    }
    case 'Div':
      return 'Dv';
    case 'BlockQuote':
      return '❝';
    case 'BulletList':
      return '•';
    case 'OrderedList':
      return '1.';
    case 'DefinitionList':
      return 'DL';
    case 'CodeBlock':
      return 'Cd';
    case 'Figure':
      return 'Fg';
    case 'Table':
      return 'Tb';
    case 'Para':
      return '¶';
    case 'Plain':
      // Distinct 2-char glyph (not the ¶ pilcrow): Plain holds [Inline] WITHOUT
      // paragraph semantics — a tight list item's leading block. Reads "• › Pl"
      // (tight) vs "• › ¶" (loose), surfacing the otherwise-invisible distinction.
      return 'Pl';
    default:
      // Fallback: first 2 characters of the type string
      return t.slice(0, 2);
  }
}

// ── categoryForSourceNode ─────────────────────────────────────────────────────

/**
 * Return the CrumbCategory for a source node. Category drives color only —
 * indent/layout behavior is keyed on node *type* elsewhere (e.g. in the
 * nesting surface logic), not on category. Do not conflate the two.
 *
 * Mirrors `labelForSourceNode`'s defensive coding style.
 */
export function categoryForSourceNode(node: BlockNode): CrumbCategory {
  if (!node || typeof node.t !== 'string') return 'leaf-text';
  const t = node.t;

  switch (t) {
    case 'Div':
      return 'container';
    case 'BulletList':
    case 'OrderedList':
    case 'DefinitionList':
      return 'list';
    case 'BlockQuote':
      return 'quote';
    case 'Para':
    case 'Plain':
    case 'Header':
      return 'leaf-text';
    case 'CodeBlock':
    case 'Figure':
    case 'Table':
      return 'embed';
    default:
      // Unlisted types (e.g. RawBlock) default to leaf-text
      return 'leaf-text';
  }
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
    abbrev: abbrevForSourceNode(sourceNode),
    category: categoryForSourceNode(sourceNode),
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

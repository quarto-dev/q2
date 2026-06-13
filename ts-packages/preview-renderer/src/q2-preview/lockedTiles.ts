import { normalizeLineEndings } from '../utils/normalizeLineEndings';
import { sliceBytes } from '../utils/sliceSource';

/**
 * lockedTiles.ts — pure DOM helper for locked-tile resolution (Plan 2b, Phase 2.2).
 *
 * A "locked tile" is the single editable surface that owns a click or keyboard
 * focus event. When multiple [data-block-pool-id] elements nest in the DOM
 * (which happens whenever a prefixing container such as a blockquote or list
 * contains inner paragraphs — each of which is itself Descendable), a click
 * identifies a root→leaf path that must be collapsed to ONE tile.
 *
 * ## Collapse precedence (single rule, two cases)
 *
 * **Case 1 — Prefixing-atomic (short-circuits Case 2):**
 *   If any element on the ancestor chain (from leaf upward) has a tag in
 *   PREFIXING_TAGS, return the OUTERMOST such element in the chain.
 *   Rationale: only the outermost prefixing container has a clean byte-slice;
 *   inner targets carry the outer `> `/indent prefix markers.
 *
 * **Case 2 — Coincidence climb (else):**
 *   Starting from the deepest [data-block-pool-id] element, climb toward the
 *   root while consecutive [data-block-pool-id] ancestors have bounding rects
 *   that coincide on all four edges within EPS_PX. The topmost coincident
 *   ancestor is the tile.
 *   Rationale: a chrome-less single-child wrapper (e.g. a bare <div> that
 *   fits its lone child exactly) coincides → you climb to it; a multi-child
 *   div or any container with visible chrome (blockquote rule, list marker
 *   gutter) does NOT coincide → you stay at the leaf.
 *
 * ## Epsilon choice (EPS_PX = 0.5)
 *   - True coincidence (0px delta on all edges) → coincides → wrapper wins.
 *   - 1px border (each edge delta = 1px, because a centered border shifts
 *     left/top by +1 and right/bottom by −1) → does NOT coincide → leaf wins.
 *   - Sub-pixel rendering jitter (<0.5px) in real browsers is tolerated.
 *   - 0.5 is the midpoint between 0 and 1, so it correctly separates the two
 *     real-world cases without ambiguity.
 *
 * ## Visibility
 *   An element with a zero-area bounding rect is hidden (e.g. a collapsed
 *   callout body inside `.callout-collapse.collapse` → Bootstrap `display:none`
 *   → zero rect). Hidden elements must never become tiles and must not
 *   participate in coincidence comparison.
 *
 * ## Idempotency
 *   resolveLockedTile(tile) === tile for any already-resolved tile. This is
 *   required so that keyboard Enter on a focused tile re-opens the same tile.
 */

/**
 * Prefixing-container tags that force Case 1.
 *
 * These map to AST node types:
 *   blockquote → BlockQuote
 *   ul         → BulletList
 *   ol         → OrderedList
 *   dl         → DefinitionList
 *
 * A paragraph INSIDE any of these carries the outer container's prefix
 * (`> ` for blockquote, list-indent for lists) in the source bytes.
 * Editing the inner paragraph's slice directly would corrupt the document,
 * so we always redirect to the outermost prefixing container.
 */
const PREFIXING_TAGS = new Set(['blockquote', 'ul', 'ol', 'dl']);

/** Epsilon (px) for the coincidence comparison. See module doc for rationale. */
const EPS_PX = 0.5;

/**
 * Returns true iff the element has a non-zero bounding rect (i.e. is visible
 * in the layout).
 *
 * In a real browser, elements with `display:none` or inside a collapsed region
 * return a zero rect. In jsdom, rects are always zero by default, so tests must
 * mock getBoundingClientRect on individual elements.
 */
export function isVisibleTile(el: Element): boolean {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
}

/**
 * Returns true iff the bounding rects of `a` and `b` coincide on all four
 * edges within `epsPx` pixels.
 *
 * The default epsilon (EPS_PX = 0.5) means:
 *   - 0px delta on every edge → true (chrome-less wrapper)
 *   - 1px border (1px delta on every edge) → false (visible chrome)
 */
export function rectsCoincide(a: Element, b: Element, epsPx: number = EPS_PX): boolean {
    const ra = a.getBoundingClientRect();
    const rb = b.getBoundingClientRect();
    return (
        Math.abs(ra.left - rb.left) <= epsPx
        && Math.abs(ra.top - rb.top) <= epsPx
        && Math.abs(ra.right - rb.right) <= epsPx
        && Math.abs(ra.bottom - rb.bottom) <= epsPx
    );
}

/**
 * Resolve a DOM element to its locked tile.
 *
 * Returns null if `el` has no [data-block-pool-id] ancestor (including itself).
 * Otherwise applies the two-case collapse described in the module doc.
 *
 * Idempotent: resolveLockedTile(tile) === tile.
 */
export function resolveLockedTile(el: Element): Element | null {
    // Step 1: find the deepest [data-block-pool-id] ancestor (includes el itself).
    const leaf = el.closest('[data-block-pool-id]');
    if (!leaf) return null;

    // Step 2: collect the full chain of [data-block-pool-id] ancestors upward,
    // from leaf to the topmost ancestor (not including the host root, which has
    // no pool-id). The chain is ordered leaf-first (index 0 = deepest).
    const chain: Element[] = [leaf];
    let current: Element = leaf;
    while (true) {
        const parent = current.parentElement?.closest('[data-block-pool-id]') ?? null;
        if (!parent) break;
        chain.push(parent);
        current = parent;
    }
    // chain[0] = leaf (deepest), chain[chain.length - 1] = outermost ancestor.

    // Step 3: Prefixing-atomic — if ANY element in the chain is a prefixing
    // container, return the OUTERMOST such element (furthest from leaf = last
    // in the chain that matches). This short-circuits the coincidence climb.
    //
    // Visibility guard: we never return a hidden element. Normally display:none
    // hides the whole subtree so a hidden prefixing ancestor would also hide
    // all descendants (making them invisible and filtered out upstream). This
    // guard is defensive — if the outer is hidden but an inner prefixing
    // element is visible, we prefer the outermost visible one. If NO prefixing
    // element is visible we fall through to the coincidence climb rather than
    // returning a hidden tile.
    let outermostPrefixing: Element | null = null;
    for (const ancestor of chain) {
        if (PREFIXING_TAGS.has(ancestor.tagName.toLowerCase()) && isVisibleTile(ancestor)) {
            // chain is leaf-to-root; iterating naturally overrides with later
            // (more outer) visible elements, so the last assignment is the
            // outermost visible prefixing element.
            outermostPrefixing = ancestor;
        }
    }
    if (outermostPrefixing !== null) {
        return outermostPrefixing;
    }

    // Step 4: Coincidence climb — starting from leaf, climb while the next
    // [data-block-pool-id] ancestor is visible and coincides with the current
    // element. The topmost reached is the tile.
    //
    // Transitivity note: we compare each ancestor against `tile` (the running
    // climb-top), not against the original leaf. This is equivalent because
    // each intermediate ancestor that passed the coincidence check has the same
    // rect as the leaf (flow-layout rects are monotone-expanding, so two
    // elements that each coincide with the leaf necessarily coincide with each
    // other). Comparing against `tile` avoids re-fetching the leaf rect.
    let tile: Element = leaf;
    for (let i = 1; i < chain.length; i++) {
        const ancestor = chain[i];
        // Skip non-visible ancestors (collapsed / display:none).
        if (!isVisibleTile(ancestor)) break;
        if (rectsCoincide(tile, ancestor)) {
            tile = ancestor;
        } else {
            // The moment a non-coincident ancestor is found, stop climbing.
            // (Further ancestors would be even larger, so they won't coincide.)
            break;
        }
    }

    return tile;
}

/**
 * Enumerate all locked tiles visible in `host`, in DOM pre-order, deduped.
 *
 * Algorithm:
 *   1. querySelectorAll('[data-block-pool-id]') — returns all pool-id elements
 *      in DOM pre-order.
 *   2. Filter to visible elements only (isVisibleTile).
 *   3. Map each through resolveLockedTile — a leaf whose tile is its parent
 *      container resolves to that container.
 *   4. Dedupe using a Set<Element>. The dedup relies on querySelectorAll's
 *      DOM pre-order guarantee (per the DOM spec, §12.4): the outer tile is
 *      enumerated before its descendants, so the Set collapses duplicates to
 *      the first occurrence = the outer tile. This is not just an artifact of
 *      the climb direction — it is guaranteed by the spec.
 *
 * This yields an ordered, deduped, visible partition of the editable surface.
 * A chrome-less single-child div and its lone child collapse to one tile
 * (the div). A multi-child div appears alongside each of its children.
 */
export function enumerateLockedTiles(host: Element): Element[] {
    const all = Array.from(host.querySelectorAll<Element>('[data-block-pool-id]'));
    const visible = all.filter(isVisibleTile);
    const seen = new Set<Element>();
    const tiles: Element[] = [];
    for (const el of visible) {
        const tile = resolveLockedTile(el);
        if (tile && !seen.has(tile)) {
            seen.add(tile);
            tiles.push(tile);
        }
    }
    return tiles;
}

// ---------------------------------------------------------------------------
// P2.3b helpers
// ---------------------------------------------------------------------------

/**
 * Type guard for Original pool entries — the only entries that represent
 * editable source blocks. Filters out generated entries (t !== 0) and
 * included-file entries (d !== 0).
 */
interface OriginalPoolEntry {
    t: 0;
    r: [number, number];
    d: 0;
}

function isOriginalEntry(entry: unknown): entry is OriginalPoolEntry {
    if (!entry || typeof entry !== 'object') return false;
    const e = entry as Record<string, unknown>;
    return e.t === 0 && e.d === 0 && Array.isArray(e.r);
}

/**
 * Find the visible locked-tile DOM element for a byte offset `anchorR0`.
 *
 * Strategy:
 *   1. Enumerate all visible locked tiles in `host` (via `enumerateLockedTiles`).
 *   2. For each tile, read its pool entry's `r[0]` from `pool[Number(tile.getAttribute('data-block-pool-id'))]`.
 *   3. Prefer an exact match (tile whose `r[0] === anchorR0`).
 *   4. If no exact match, return the tile with the smallest `r[0] >= anchorR0`
 *      (nearest-at/after) — unless `exactOnly` is true (see below).
 *   5. Return null if no visible tile qualifies.
 *
 * Hidden tiles (zero rect, `isVisibleTile` returns false) are excluded by
 * `enumerateLockedTiles`. This means a hidden tile that would be an exact
 * match is skipped; the next visible tile at/after is tried — UNLESS
 * `exactOnly` is true, in which case null is returned.
 *
 * ### `exactOnly` option
 *
 * When `opts.exactOnly` is `true`, the nearest-at/after fallback is suppressed.
 * The function returns the visible tile only if its `r[0] === anchorR0` exactly;
 * otherwise null. This is required for the hidden-surface drop check in the
 * P2.3b self-heal effect:
 *
 *   - After a successful re-anchor to `cand.r0`, we need to know whether the
 *     tile at exactly `cand.r0` is visible.
 *   - With the default path, if the re-anchored tile is hidden (excluded by
 *     `enumerateLockedTiles`) but a later visible tile exists, `tileForAnchorR0`
 *     returns that later tile (non-null) — causing the hidden-drop to be missed.
 *   - `exactOnly: true` returns null in that case, correctly triggering the drop.
 *
 * Drop-focus (P2.4) uses the default path (no `exactOnly`) where an approximate
 * nearest landing is acceptable.
 */
export function tileForAnchorR0(
    host: Element,
    pool: unknown[],
    anchorR0: number,
    opts?: { exactOnly?: boolean },
): Element | null {
    const tiles = enumerateLockedTiles(host);
    const exactOnly = opts?.exactOnly ?? false;

    let exactTile: Element | null = null;
    let nearestTile: Element | null = null;
    let nearestR0 = Infinity;

    for (const tile of tiles) {
        const poolIdAttr = tile.getAttribute('data-block-pool-id');
        if (poolIdAttr === null) continue;
        const poolId = Number(poolIdAttr);
        const entry = pool[poolId];
        if (!isOriginalEntry(entry)) continue;
        const r0 = entry.r[0];

        if (r0 === anchorR0) {
            exactTile = tile;
            break;  // exact is highest priority; stop searching
        }

        if (!exactOnly && r0 > anchorR0 && r0 < nearestR0) {
            nearestR0 = r0;
            nearestTile = tile;
        }
    }

    return exactTile ?? (exactOnly ? null : nearestTile);
}

/**
 * Find a pool entry to re-anchor the active editor to after a collaborator's
 * re-render. Returns `{ r0, r1 }` if re-anchoring is possible, or `null` if
 * the block should be dropped.
 *
 * Algorithm:
 *   1. Filter pool to Original entries (t=0, d=0).
 *   2. Try exact: an entry whose `r[0] === anchorR0`.
 *   3. If no exact, try nearest: the entry with the smallest `r[0] >= anchorR0`.
 *   4. Apply content-verify to whichever candidate was found:
 *      `normalizeLineEndings(sliceBytes(content, cand.r[0], cand.r[1])).trimEnd() === anchorSlice`.
 *   5. Return `{ r0: cand.r[0], r1: cand.r[1] }` on success, `null` on failure
 *      (no candidate, or content mismatch).
 *
 * The content-verify is the ACTUAL arbiter — even if an exact entry exists at
 * `anchorR0`, it must still pass the content check because a collaborator
 * could have inserted a different block at that exact byte offset.
 *
 * Note: `exact ?? nearest` means if an exact entry exists AND fails the
 * content check, we return null (do NOT fall back to nearest). This is
 * intentional: if there IS an entry at the old anchorR0 but with different
 * content, it is a different block that happened to land at the same offset —
 * continuing with the nearest would mis-target.
 */
export function findReanchorCandidate(
    pool: unknown[],
    content: string,
    anchorR0: number,
    anchorSlice: string,
): { r0: number; r1: number } | null {
    const orig = (pool as unknown[]).filter(isOriginalEntry);

    const exact = orig.find((e) => e.r[0] === anchorR0);
    const atAfter = orig.filter((e) => e.r[0] >= anchorR0);
    const nearest = atAfter.length > 0
        ? atAfter.reduce((a, b) => b.r[0] < a.r[0] ? b : a)
        : undefined;

    const cand = exact ?? nearest;
    if (!cand) return null;

    const sliced = normalizeLineEndings(sliceBytes(content, cand.r[0], cand.r[1])).trimEnd();
    if (sliced === anchorSlice) {
        return { r0: cand.r[0], r1: cand.r[1] };
    }
    return null;
}

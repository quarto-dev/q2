import { normalizeLineEndings } from '../utils/normalizeLineEndings';
import { sliceBytes } from '../utils/sliceSource';
import { serializeSourceEntry } from './sourceIndex';

/**
 * outerBlocks.ts — pure DOM helper for outer-block resolution (Plan 2b, Phase 2.2).
 *
 * An "outer block" is the single editable surface that owns a click or keyboard
 * focus event. When multiple [data-block-pool-id] elements nest in the DOM
 * (which happens whenever a prefixing container such as a blockquote or list
 * contains inner paragraphs — each of which is itself Descendable), a click
 * identifies a root→leaf path that must be collapsed to ONE outer block.
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
 *   ancestor is the outer block.
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
 *   → zero rect). Hidden elements must never become outer blocks and must not
 *   participate in coincidence comparison.
 *
 * ## Idempotency
 *   resolveOuterBlock(outerBlock) === outerBlock for any already-resolved outer block. This is
 *   required so that keyboard Enter on a focused outer block re-opens the same outer block.
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
export function isVisibleBlock(el: Element): boolean {
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
 * Resolve a DOM element to its outer block.
 *
 * Returns null if `el` has no [data-block-pool-id] ancestor (including itself).
 * Otherwise applies the two-case collapse described in the module doc.
 *
 * Idempotent: resolveOuterBlock(outerBlock) === outerBlock.
 */
export function resolveOuterBlock(el: Element): Element | null {
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
    // returning a hidden outer block.
    let outermostPrefixing: Element | null = null;
    for (const ancestor of chain) {
        if (PREFIXING_TAGS.has(ancestor.tagName.toLowerCase()) && isVisibleBlock(ancestor)) {
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
    // element. The topmost reached is the outer block.
    //
    // Transitivity note: we compare each ancestor against `outerBlock` (the running
    // climb-top), not against the original leaf. This is equivalent because
    // each intermediate ancestor that passed the coincidence check has the same
    // rect as the leaf (flow-layout rects are monotone-expanding, so two
    // elements that each coincide with the leaf necessarily coincide with each
    // other). Comparing against `outerBlock` avoids re-fetching the leaf rect.
    let outerBlock: Element = leaf;
    for (let i = 1; i < chain.length; i++) {
        const ancestor = chain[i];
        // Skip non-visible ancestors (collapsed / display:none).
        if (!isVisibleBlock(ancestor)) break;
        if (rectsCoincide(outerBlock, ancestor)) {
            outerBlock = ancestor;
        } else {
            // The moment a non-coincident ancestor is found, stop climbing.
            // (Further ancestors would be even larger, so they won't coincide.)
            break;
        }
    }

    return outerBlock;
}

/**
 * Enumerate all outer blocks visible in `host`, in DOM pre-order, deduped.
 *
 * Algorithm:
 *   1. querySelectorAll('[data-block-pool-id]') — returns all pool-id elements
 *      in DOM pre-order.
 *   2. Filter to visible elements only (isVisibleBlock).
 *   3. Map each through resolveOuterBlock — a leaf whose outer block is its parent
 *      container resolves to that container.
 *   4. Dedupe using a Set<Element>. The dedup relies on querySelectorAll's
 *      DOM pre-order guarantee (per the DOM spec, §12.4): the outer block is
 *      enumerated before its descendants, so the Set collapses duplicates to
 *      the first occurrence = the outer block. This is not just an artifact of
 *      the climb direction — it is guaranteed by the spec.
 *
 * This yields an ordered, deduped, visible partition of the editable surface.
 * A chrome-less single-child div and its lone child collapse to one outer block
 * (the div). A multi-child div appears alongside each of its children.
 */
export function enumerateOuterBlocks(host: Element): Element[] {
    const all = Array.from(host.querySelectorAll<Element>('[data-block-pool-id]'));
    const visible = all.filter(isVisibleBlock);
    const seen = new Set<Element>();
    const blocks: Element[] = [];
    for (const el of visible) {
        const outerBlock = resolveOuterBlock(el);
        if (outerBlock && !seen.has(outerBlock)) {
            seen.add(outerBlock);
            blocks.push(outerBlock);
        }
    }
    return blocks;
}

/**
 * Enumerate the strict DOM-leaf nesting surfaces of `host` — visible
 * `[data-block-pool-id]` elements that have **no** `[data-block-pool-id]`
 * descendant — in DOM pre-order.
 *
 * This is the **strict leaf** variant. It is NOT the roving enumerator for
 * §2 unlock mode — use `enumerateNestingSurfaces` for roving.
 *
 * Why `enumerateNestingSurfaces` is the roving enumerator (and this is not):
 *   A multi-block `<li>`/`<dd>` proxy (text + sublist) owns leading text but
 *   also has a nested `[data-block-pool-id]` descendant (the sublist).
 *   `enumerateNestingLeaves` would DROP that proxy (fails the "no pool-id
 *   descendant" test), making its leading text unreachable by keyboard roving.
 *   `enumerateNestingSurfaces` re-includes `<li>`/`<dd>` proxies via its
 *   LI/DD clause (case b), so all landable surfaces remain reachable.
 *
 * Use this function only when you specifically need strict DOM-leaves (no
 * container with nested pool-id children). See `enumerateNestingSurfaces`
 * for the §2 C1 partition that roving-tabindex uses.
 *
 * Pure (no React); visibility via `isVisibleBlock`.
 */
export function enumerateNestingLeaves(host: Element): Element[] {
    const all = Array.from(host.querySelectorAll<Element>('[data-block-pool-id]'));
    return all.filter(
        (el) => isVisibleBlock(el) && el.querySelector('[data-block-pool-id]') === null,
    );
}

/**
 * §2: enumerate the C1 visible-surface partition of `host` — the landable
 * surfaces in unlock mode — in DOM pre-order.
 *
 * Definition: include `el` iff `isVisibleBlock(el)` AND one of:
 *   (a) `el` has no `[data-block-pool-id]` descendant — it is a DOM-leaf
 *       (same as `enumerateNestingLeaves`); OR
 *   (b) `el.tagName` is `LI` or `DD` — it is a §0 proxy that owns leading
 *       text before its first nested block.
 *
 * Rationale for the LI/DD clause:
 *   A single-block `<li>` has no pool-id descendant → already a DOM-leaf (case a).
 *   A multi-block `<li>`/`<dd>` proxy (text + sublist) has the sublist as a
 *   descendant pool-id element but OWNS its leading text (the Plain block whose
 *   id was borrowed). Without the LI/DD clause, `enumerateNestingLeaves` would
 *   drop the multi-block proxy (it has a pool-id descendant), making the leading
 *   text unreachable by roving. The LI/DD clause re-includes it.
 *
 * Pure containers are correctly EXCLUDED:
 *   - A `<ul>`/`<ol>`/`<dl>`/`<blockquote>` with pool-id children: NOT LI/DD,
 *     HAS pool-id descendants → excluded (case a fails, case b fails).
 *
 * Consistency with §1's navigable surfaces:
 *   §1's UNLOCK resolver (`resolveLanding` in PreviewRoot.tsx) uses
 *   `buildNestingSurfaces(sourceIndex)` — the source-index partition — and then
 *   searches `querySelectorAll('[data-block-pool-id]')` for the DOM element whose
 *   pool entry matches the resolved surface's byte range. `buildNestingSurfaces`
 *   includes §0 proxies (list item leading blocks), so both surfaces agree:
 *   this DOM enumerator includes the LI/DD proxies that §1 can land on.
 *
 * Returns elements in DOM pre-order (same order `querySelectorAll` yields).
 * Pure (no React); visibility via `isVisibleBlock`.
 */
export function enumerateNestingSurfaces(host: Element): Element[] {
    const all = Array.from(host.querySelectorAll<Element>('[data-block-pool-id]'));
    return all.filter((el) => {
        if (!isVisibleBlock(el)) return false;
        const hasPoolDescendant = el.querySelector('[data-block-pool-id]') !== null;
        if (!hasPoolDescendant) return true; // DOM-leaf (case a)
        return el.tagName === 'LI' || el.tagName === 'DD'; // §0 proxy with leading text (case b)
    });
}

// ---------------------------------------------------------------------------
// P2.4b helpers
// ---------------------------------------------------------------------------

/**
 * Measure the content height and full computed box style of a block element.
 *
 * Extracted from `useBlockEditHover.tsx`'s `activate` function so the same
 * measurement can be used both at click-activation time and at move-open time
 * (sync hop and reland paths in `entry.tsx`).
 *
 * - `contentHeight`: rect.height minus padding and border (content-area height).
 *   Used as the textarea height so it fills the content area exactly.
 * - `boxStyle`: full computed box (margin + padding + per-side border longhands)
 *   for the measure-and-set wrapper to replicate the element's exact box.
 *
 * ## Delegation
 *
 * This function delegates entirely to `measureLeadingBlockBox`, which provides:
 * - Range-aware measurement for `<li>` / `<dd>` elements with nested
 *   `[data-block-pool-id]` children (Amendment A4 — captures only the leading
 *   text run, not the sublist height).
 * - Element-rect measurement for all other elements.
 * - jsdom-safe behaviour (never throws; `contentHeight` is 0 in zero-layout
 *   environments). See `measureLeadingBlockBox` for the full jsdom guarantees.
 *
 * The `rangeUsed` diagnostic flag returned by `measureLeadingBlockBox` is
 * stripped before returning — it is an internal test-only detail.
 */
export function measureBlockBox(blockEl: Element): {
    contentHeight: number;
    boxStyle: Record<string, string>;
} {
    // Delegate to measureLeadingBlockBox, which handles:
    //   - Range-aware measurement for <li>/<dd> with nested pool-id children (Amendment A4)
    //   - Element-rect measurement for all other elements
    // Strip the diagnostic `rangeUsed` flag before returning.
    const { contentHeight, boxStyle } = measureLeadingBlockBox(blockEl);
    return { contentHeight, boxStyle };
}

/**
 * Measure the content height of an `<li>` or `<dd>` element's LEADING block
 * (the text run before any nested block children), or fall back to the element's
 * own box for non-list elements.
 *
 * ## Why a Range?
 *
 * A tight list item whose leading block is a `Plain` borrows the Plain's pool-id
 * onto the `<li>` element. When the item also contains a nested block (e.g. a
 * sub-list), the `<li>`'s `getBoundingClientRect()` includes the sublist height —
 * but the editor should only occupy the leading text run's extent. A DOM `Range`
 * from the element's start to its first `[data-block-pool-id]` descendant
 * (exclusive) captures exactly the leading text run.
 *
 * ## When Range is used
 *
 * 1. `blockEl` is an `<li>` or `<dd>`.
 * 2. The element has at least one `[data-block-pool-id]` descendant.
 *
 * Otherwise, the element's own box is measured (identical to `measureBlockBox`).
 *
 * ## jsdom behaviour
 *
 * `Range.getBoundingClientRect()` always returns a zero rect in jsdom; tests
 * must stub `Range.prototype.getBoundingClientRect` to assert the Range path.
 * The function never throws in a zero-layout environment — `contentHeight`
 * will simply be 0.
 *
 * Returns `{ contentHeight, boxStyle, rangeUsed }`. `rangeUsed` is a diagnostic
 * flag exposed only for unit tests to assert which measurement path was taken.
 * `measureBlockBox` strips this flag before returning; it does NOT read it to
 * dispatch between paths.
 *
 * This is an internal helper called by `measureBlockBox`. It is exported only for
 * the 0.a unit tests that need to assert `rangeUsed` directly. Production code
 * should call `measureBlockBox` instead.
 */
export function measureLeadingBlockBox(blockEl: Element): {
    contentHeight: number;
    boxStyle: Record<string, string>;
    rangeUsed: boolean;
} {
    const tagName = blockEl.tagName.toLowerCase();
    const isListItem = tagName === 'li' || tagName === 'dd';
    const nestedPoolChild = isListItem
        ? blockEl.querySelector('[data-block-pool-id]')
        : null;

    if (isListItem && nestedPoolChild !== null) {
        // Range path: measure from the element's start to its first nested
        // block child (exclusive), capturing only the leading text run.
        const range = document.createRange();
        range.setStart(blockEl, 0);
        range.setEndBefore(nestedPoolChild);
        // jsdom does not implement Range.getBoundingClientRect — guard gracefully.
        const rangeRect: DOMRect | null = typeof range.getBoundingClientRect === 'function'
            ? range.getBoundingClientRect()
            : null;
        const px = (v: string) => parseFloat(v) || 0;
        // A Plain fragment has no padding/border of its own; the leading-text
        // contentHeight is simply the range rect height.
        const contentHeight = rangeRect !== null ? px(String(rangeRect.height)) : 0;
        // The boxStyle for the range proxy is the trivial/empty box — no
        // padding or border on the Plain fragment itself.
        const emptyBoxStyle: Record<string, string> = {
            marginTop: '', marginRight: '', marginBottom: '', marginLeft: '',
            paddingTop: '', paddingRight: '', paddingBottom: '', paddingLeft: '',
            borderTopWidth: '', borderRightWidth: '', borderBottomWidth: '', borderLeftWidth: '',
            borderTopStyle: '', borderRightStyle: '', borderBottomStyle: '', borderLeftStyle: '',
            borderTopColor: '', borderRightColor: '', borderBottomColor: '', borderLeftColor: '',
        };
        return { contentHeight, boxStyle: emptyBoxStyle, rangeUsed: true };
    }

    // Element path: measure the element's bounding rect minus padding/border,
    // identical to the element-rect branch in measureBlockBox.
    const elRect = blockEl.getBoundingClientRect();
    const cs = getComputedStyle(blockEl);
    const px = (v: string) => parseFloat(v) || 0;
    const contentHeight = elRect.height
        - px(cs.paddingTop) - px(cs.paddingBottom)
        - px(cs.borderTopWidth) - px(cs.borderBottomWidth);
    const boxStyle: Record<string, string> = {
        marginTop: cs.marginTop, marginRight: cs.marginRight,
        marginBottom: cs.marginBottom, marginLeft: cs.marginLeft,
        paddingTop: cs.paddingTop, paddingRight: cs.paddingRight,
        paddingBottom: cs.paddingBottom, paddingLeft: cs.paddingLeft,
        borderTopWidth: cs.borderTopWidth, borderRightWidth: cs.borderRightWidth,
        borderBottomWidth: cs.borderBottomWidth, borderLeftWidth: cs.borderLeftWidth,
        borderTopStyle: cs.borderTopStyle, borderRightStyle: cs.borderRightStyle,
        borderBottomStyle: cs.borderBottomStyle, borderLeftStyle: cs.borderLeftStyle,
        borderTopColor: cs.borderTopColor, borderRightColor: cs.borderRightColor,
        borderBottomColor: cs.borderBottomColor, borderLeftColor: cs.borderLeftColor,
    };
    return { contentHeight, boxStyle, rangeUsed: false };
}

/**
 * Capture the edit identity triple from an outer block element.
 *
 * Reads the `data-block-pool-id` attribute off `blockEl`, looks up the pool
 * entry, slices `content` to get the source text, and returns the identity
 * triple `{ anchorR0, anchorR1, anchorSlice }` used to open an editor.
 *
 * Returns `null` when:
 *  - `blockEl` has no `data-block-pool-id` attribute.
 *  - The pool entry is missing or not an Original entry (`t !== 0`).
 *
 * This is the canonical place for the identity logic. Both `activate` in
 * `useBlockEditHover.tsx` (click/keyboard) and the nav reland in `entry.tsx`
 * (arrow-key move) use this helper instead of inlining the same calculation.
 *
 * Box measurements (`contentHeight`, `boxStyle`) are NOT included — those
 * require live DOM geometry and belong at the activation site.
 */
export function captureEditTarget(
    blockEl: Element,
    pool: unknown[],
    content: string,
): { anchorR0: number; anchorR1: number; anchorSlice: string } | null {
    const pidAttr = blockEl.getAttribute('data-block-pool-id');
    if (pidAttr === null) return null;

    const poolEntry = pool[Number(pidAttr)] as { t: number; r: [number, number]; d: number } | undefined;
    if (!poolEntry || poolEntry.t !== 0) return null;

    const anchorR0 = poolEntry.r[0];
    const anchorR1 = poolEntry.r[1];
    const anchorSlice = normalizeLineEndings(sliceBytes(content, anchorR0, anchorR1)).trimEnd();

    return { anchorR0, anchorR1, anchorSlice };
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
 * Find the visible outer-block DOM element for a byte offset `anchorR0`.
 *
 * Strategy:
 *   1. Enumerate all visible outer blocks in `host` (via `enumerateOuterBlocks`).
 *   2. For each outer block, read its pool entry's `r[0]` from `pool[Number(outerBlock.getAttribute('data-block-pool-id'))]`.
 *   3. Prefer an exact match (outer block whose `r[0] === anchorR0`).
 *   4. If no exact match, return the outer block with the smallest `r[0] >= anchorR0`
 *      (nearest-at/after) — unless `exactOnly` is true (see below).
 *   5. Return null if no visible outer block qualifies.
 *
 * Hidden outer blocks (zero rect, `isVisibleBlock` returns false) are excluded by
 * `enumerateOuterBlocks`. This means a hidden outer block that would be an exact
 * match is skipped; the next visible outer block at/after is tried — UNLESS
 * `exactOnly` is true, in which case null is returned.
 *
 * ### `exactOnly` option
 *
 * When `opts.exactOnly` is `true`, the nearest-at/after fallback is suppressed.
 * The function returns the visible outer block only if its `r[0] === anchorR0` exactly;
 * otherwise null. This is required for the hidden-surface drop check in the
 * P2.3b self-heal effect:
 *
 *   - After a successful re-anchor to `cand.r0`, we need to know whether the
 *     outer block at exactly `cand.r0` is visible.
 *   - With the default path, if the re-anchored outer block is hidden (excluded by
 *     `enumerateOuterBlocks`) but a later visible outer block exists, `outerBlockForAnchorR0`
 *     returns that later outer block (non-null) — causing the hidden-drop to be missed.
 *   - `exactOnly: true` returns null in that case, correctly triggering the drop.
 *
 * Drop-focus (P2.4) uses the default path (no `exactOnly`) where an approximate
 * nearest landing is acceptable.
 */
export function outerBlockForAnchorR0(
    host: Element,
    pool: unknown[],
    anchorR0: number,
    opts?: { exactOnly?: boolean },
): Element | null {
    const blocks = enumerateOuterBlocks(host);
    const exactOnly = opts?.exactOnly ?? false;

    let exactBlock: Element | null = null;
    let nearestBlock: Element | null = null;
    let nearestR0 = Infinity;

    for (const outerBlock of blocks) {
        const poolIdAttr = outerBlock.getAttribute('data-block-pool-id');
        if (poolIdAttr === null) continue;
        const poolId = Number(poolIdAttr);
        const entry = pool[poolId];
        if (!isOriginalEntry(entry)) continue;
        const r0 = entry.r[0];

        if (r0 === anchorR0) {
            exactBlock = outerBlock;
            break;  // exact is highest priority; stop searching
        }

        if (!exactOnly && r0 > anchorR0 && r0 < nearestR0) {
            nearestR0 = r0;
            nearestBlock = outerBlock;
        }
    }

    return exactBlock ?? (exactOnly ? null : nearestBlock);
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

/**
 * Seed the draft buffer for a source range — the canonical, shared seeding step
 * used by every fresh-open / reland / nest-retarget site.
 *
 * Returns:
 *  - `anchorSlice`: the raw source text of `[range.r0, range.r1)`, line-ending
 *    normalized and right-trimmed (identical to what `captureEditTarget`
 *    produces for the same range).
 *  - `seededDraft`: the clean nesting buffer for the range if one exists
 *    (`nestedEditBuffers[siKey]`), else `anchorSlice`. The clean buffer strips
 *    the ancestor `> `/indent prefix; top-level blocks have no entry and fall
 *    back to `anchorSlice`.
 *
 * Extracted from the duplicated 2-line seeding in `activate`
 * (useBlockEditHover) and `applyNestingRetarget` (PreviewRoot) so both share one
 * definition. Pure (no DOM, no React).
 */
export function seedForRange(
    range: { r0: number; r1: number },
    content: string,
    nestedEditBuffers: Record<string, string> | null | undefined,
): { anchorSlice: string; seededDraft: string } {
    const anchorSlice = normalizeLineEndings(sliceBytes(content, range.r0, range.r1)).trimEnd();
    const siKey = serializeSourceEntry({ t: 0, r: [range.r0, range.r1], d: 0 });
    const seededDraft = nestedEditBuffers?.[siKey] ?? anchorSlice;
    return { anchorSlice, seededDraft };
}

/**
 * The canonical dirty baseline for an open edit target: the value the draft was
 * seeded with at open (`seededDraft`), falling back to `anchorSlice` for
 * non-nested blocks (or pre-P3.3 activation paths that never set `seededDraft`).
 *
 * G19 (Layer 2): this is the SINGLE source of the baseline. For a nested block
 * the raw `anchorSlice` carries the ancestor `> `/indent prefix, while the clean
 * `seededDraft` does not — comparing a draft against the raw slice reads an
 * untouched clean-buffer editor as dirty. Every draft-vs-baseline comparison
 * MUST route through this helper so no site can drift to the raw slice again.
 */
export function editBaseline(et: { seededDraft?: string; anchorSlice: string }): string {
    return normalizeLineEndings(et.seededDraft ?? et.anchorSlice).trimEnd();
}

/**
 * Whether `draft` differs from the edit target's canonical baseline, treating an
 * empty draft as NOT dirty (the policy shared by the click-switch + nesting-move
 * sites). Sites with delete-by-emptying semantics (commitIfDirty, arrow step-off)
 * keep their own three-way empty handling but still source the baseline from
 * `editBaseline`.
 */
export function isDirty(draft: string, et: { seededDraft?: string; anchorSlice: string }): boolean {
    const draftNorm = normalizeLineEndings(draft).trimEnd();
    return !!draftNorm && draftNorm !== editBaseline(et);
}

/**
 * §1 geometry snapshot — capture the rendered geometry of an opened block's
 * whole top-level subtree, keyed BLOCK-RELATIVE to the top-level block's start
 * byte, so a later nest move can size its destination from the ORIGINAL render
 * instead of the edit-distorted live DOM (where the active subtree is a textarea).
 *
 * Must run synchronously at activation, BEFORE the children are swapped to a
 * textarea.
 *
 *  1. Climb from `openedEl` to the OUTERMOST `[data-block-pool-id]` (the
 *     top-level block) via the same chain-walk `resolveOuterBlock` uses.
 *  2. Collect `[topBlock, ...topBlock descendants with a pool id]`. If ≤ 1 is
 *     visible (a flat block, no nesting to size) → return an empty map.
 *  3. Otherwise `measureBlockBox` each visible surface, keyed
 *     `` `${pool[pid].r[0] - topBlockR0}:${pool[pid].r[1] - topBlockR0}` `` — the
 *     full `(r0,r1)` range is the only thing that joins a transformed DOM element
 *     to an untransformed nesting surface; `r0`-only collides for a container
 *     whose first child shares its `r0`. Block-relative subtraction makes the key
 *     shift-invariant under an insert-above.
 *
 * Duplicate-key rule: DOM-pre-order-first (outermost wins), matching
 * `enumerateOuterBlocks`' dedupe; coincident same-range wrappers are harmless
 * (same box).
 *
 * `topBlockR0` is supplied by the caller (derived from the source index via
 * `nestingNav.topBlockR0`) so capture and lookup subtract the SAME origin.
 */
export function snapshotOuterBlockGeometry(
    openedEl: Element,
    pool: Array<{ t: number; r: [number, number]; d: number }>,
    topBlockR0: number,
): Map<string, { contentHeight: number; boxStyle: Record<string, string> }> {
    const empty = new Map<string, { contentHeight: number; boxStyle: Record<string, string> }>();

    // Step 1: climb to the outermost [data-block-pool-id].
    const leaf = openedEl.closest('[data-block-pool-id]');
    if (!leaf) return empty;
    let topBlock: Element = leaf;
    let current: Element = leaf;
    while (true) {
        const parent = current.parentElement?.closest('[data-block-pool-id]') ?? null;
        if (!parent) break;
        topBlock = parent;
        current = parent;
    }

    // Step 2: the top block plus its pool-id descendants; gate on >1 visible.
    const surfaces: Element[] = [
        topBlock,
        ...Array.from(topBlock.querySelectorAll<Element>('[data-block-pool-id]')),
    ];
    const visible = surfaces.filter(isVisibleBlock);
    if (visible.length <= 1) return empty;

    // Step 3: measure each, key block-relative, DOM-pre-order-first on duplicates.
    const map = new Map<string, { contentHeight: number; boxStyle: Record<string, string> }>();
    for (const el of visible) {
        const pidAttr = el.getAttribute('data-block-pool-id');
        if (pidAttr === null) continue;
        const entry = pool[Number(pidAttr)];
        if (!entry || !Array.isArray(entry.r)) continue;
        const key = `${entry.r[0] - topBlockR0}:${entry.r[1] - topBlockR0}`;
        if (map.has(key)) continue; // outermost (pre-order-first) wins
        map.set(key, measureBlockBox(el));
    }
    return map;
}

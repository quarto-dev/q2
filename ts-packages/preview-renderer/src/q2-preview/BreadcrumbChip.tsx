/**
 * BreadcrumbChip.tsx — floating ancestor-path breadcrumb for the nesting cursor.
 *
 * P3.4 §3d: shows the AST ancestor path (e.g. "Section › Div › Paragraph")
 * with the current level highlighted. Rendered when unlockNestingCursor=true
 * AND an editor is open (editTarget != null). Self-gating: renders null
 * otherwise.
 *
 * Pointer-isolation note: stopPropagation/preventDefault are implemented here
 * for correct real-browser behaviour (prevent host click-switch; prevent blur-
 * commit on button press). jsdom's fireEvent.click does not simulate pointer
 * events or focus/blur, so these behaviours are NOT jsdom-tested here.
 * Pointer-isolation testing is deferred to P3.5 Playwright.
 *
 * ## Positioning model (Phase 3)
 *
 * Mount host: `#quarto-content` (a `page-columns` grid container spanning
 * screen-start → screen-end). The chip is `position:absolute` inside it.
 *
 * `#quarto-content` is given `position: relative` via the injected stylesheet
 * so it becomes the chip's offset-parent. Both the chip's containing block and
 * the active surface then scroll together with `#root { overflow:auto }` — a
 * once-computed offset remains scroll-stable with no listener required.
 *
 * Stacking-context note: `position: relative` on `#quarto-content` creates a
 * stacking context. Risk rated LOW (no `position:fixed` children inside
 * #quarto-content; sidebar and TOC overlays have their own contexts above the
 * page grid). The chip's `z-index:50` is intentional: paint above sidebar(z≈1)
 * and main(z=0) but below any modal/dialog. See §Positioning model in the plan.
 *
 * ## Layout model (pivot-pinned left-spill, Phase 3b/3c + G1)
 *
 * The crumb row's RIGHT edge is pinned at the pivot (the active surface's left
 * edge, `surfaceLeft`). The band is sized to the *comfortable* width the whole
 * path wants — `naturalWidth = crumbCount * CRUMB_W` — not the legibility floor.
 * When the indent gutter [colLeft, surfaceLeft] is wide enough to hold that, the
 * band fills the gutter exactly (no change from the old model). When it isn't —
 * a shallow nesting, or a container that contributes no indent of its own (code
 * block, non-indenting Div) — the excess band width pushes the chip's left edge
 * LEFT into the page margin. Crumbs DO enter the margin in that case; that is the
 * "where you came from" direction. ◀ rides the chip's left edge.
 *
 *   page edge          text-col margin (colLeft)      surface-left (pivot)
 *   | ◀  ❝  Cd |                                  [ editing surface text … ]
 *        └─ shallow path: band spilled left past colLeft into the margin ─┘
 *
 * Hard stop (left page edge): if the left-spill would cross `#quarto-content`'s
 * left edge (x < 0) the chip would leave `#root`'s content box and add a horizontal
 * scrollbar — so ◀ is pinned at x=0 and, rather than crunching, the band keeps its
 * comfortable width and spills RIGHT past the pivot (over the content, which has
 * room). The pure math lives in `computeChipGeometry`.
 */

import React, { useContext, useLayoutEffect, useRef, useState } from 'react';
import { PreviewContext } from './PreviewContext';
import { buildAncestorPath, detectPlatform } from './nestingNav';
import type { AncestorCrumb } from './nestingNav';

// ── Constants ──────────────────────────────────────────────────────────────────

/**
 * Minimum legible width (px) for the ◀ button and the LEGIBILITY FLOOR used to
 * count how many crumbs fit a band before the middle must ellipsize. Most crumb
 * glyphs are a single character (•, ¶, H2, Dv, 1.) at 12px font with ~3px padding
 * each side, so ~16px is the smallest that stays legible. NOTE: this is only the
 * floor for the slot count / page-edge ellipsize decision — the *comfortable*
 * per-crumb width used to size the band is `CRUMB_W`.
 */
export const MIN_GLYPH_W = 16;

/**
 * Comfortable per-crumb width (px) used to size the crumb band (`naturalWidth =
 * crumbCount * CRUMB_W`) so a shallow / zero-indent path spills LEFT into the
 * margin at a readable width rather than crunching to the `MIN_GLYPH_W` floor.
 *
 * LIVE-TUNED (G1 step 3): the value is a visual judgement, not a correctness
 * gate — `computeChipGeometry`'s tests derive their expectations from this
 * constant, so changing it cannot break them. Tune against a code-block-in-
 * blockquote (must show both crumbs without crunch) and a 3-level list (must not
 * over-spill past the page's left edge). Floor is `MIN_GLYPH_W` (16); start ~26.
 */
export const CRUMB_W = 22;

// ── Display item types ─────────────────────────────────────────────────────────

// The crumb row fills a fixed band [◀.right, surfaceLeft] via flexbox, so items
// carry no explicit width — they share the band equally (expand when few, shrink
// when many; the middle ellipsizes once they no longer fit at MIN_GLYPH_W).
type CrumbDisplayItem =
    | { kind: 'crumb'; crumb: AncestorCrumb }
    | { kind: 'ellipsis' };

// ── Chip geometry (computed once per editTarget change) ────────────────────────

interface ChipGeometry {
    /** chip top (px, relative to #quarto-content). */
    top: number;
    /** left edge of the chip (px, relative to #quarto-content), from
     *  `computeChipGeometry`. Equals `surfaceLeft − OUT_W − bandWidth` (right edge
     *  pinned at the pivot), so for a shallow / zero-indent path it sits LEFT of
     *  the text column (crumbs spill into the margin); clamped to 0 at the page
     *  edge. For a deep indent it reduces to `colLeft − OUT_W` (old behavior). */
    chipLeft: number;
    /** Width (px) of the crumb band, from `computeChipGeometry`:
     *  `max(gutter, crumbCount * CRUMB_W, MIN_GLYPH_W)` — the comfortable natural
     *  width unless the gutter is already wider. The crumbs flex to fill it; the
     *  band's right edge meets the pivot, EXCEPT when ◀ is pinned at the left page
     *  edge (x=0), where the band keeps this width and spills right past the pivot. */
    bandWidth: number;
    /** Items for the crumb row (may include a single ellipsis when the path is too
     *  long to show every crumb at a legible width). */
    displayItems: CrumbDisplayItem[];
}

/** Select which crumbs to show given how many MIN_GLYPH_W slots fit the band. */
function selectDisplayItems(crumbs: AncestorCrumb[], slots: number): CrumbDisplayItem[] {
    const n = crumbs.length;
    if (n === 0) return [];
    if (slots >= n) return crumbs.map((crumb) => ({ kind: 'crumb', crumb }));
    if (slots <= 1) return [{ kind: 'crumb', crumb: crumbs[n - 1] }]; // current only
    if (slots === 2) {
        // root + current
        return [{ kind: 'crumb', crumb: crumbs[0] }, { kind: 'crumb', crumb: crumbs[n - 1] }];
    }
    // root … current
    return [
        { kind: 'crumb', crumb: crumbs[0] },
        { kind: 'ellipsis' },
        { kind: 'crumb', crumb: crumbs[n - 1] },
    ];
}

/**
 * Pure chip geometry (G1 — pivot-pinned left-spill). Inputs are host-relative px:
 * `surfaceLeft` (the pivot = active surface's left edge), `colLeft` (text-column
 * left margin), and the crumb count. Returns the chip's left edge, the crumb-band
 * width, and the slot count for `selectDisplayItems`.
 *
 * The band's RIGHT edge is always pinned at the pivot (`surfaceLeft`); the band is
 * sized to the *comfortable* `naturalWidth = crumbCount * CRUMB_W` (not the
 * legibility floor), so when a container contributes little/no indent gutter the
 * excess width pushes the chip's left edge LEFT into the page margin — the "where
 * you came from" direction — instead of collapsing crumbs. Invariants:
 *   - Deep indent (`gutter ≥ naturalWidth`): `bandWidth == gutter`,
 *     `chipLeft == colLeft − OUT_W` — identical to the old gutter-only model.
 *   - Out of room on the left (chipLeft < 0): pin ◀ at the page edge (x=0) and keep
 *     the comfortable band width, letting it spill RIGHT past the pivot (over the
 *     content) rather than crunching/ellipsizing — the right side has room, so this
 *     adds no horizontal scrollbar. The right-edge-at-pivot invariant is relaxed
 *     here by design.
 *   - Unmeasured layout (`surfaceLeft ≤ 0`, e.g. jsdom zero-rects): keep the old
 *     anchor and report `slots == crumbCount` so the full path still renders.
 */
export function computeChipGeometry(
    surfaceLeft: number,
    colLeft: number,
    crumbCount: number,
): { chipLeft: number; bandWidth: number; slots: number } {
    const OUT_W = MIN_GLYPH_W;
    const gutter = surfaceLeft - colLeft;
    const naturalWidth = crumbCount * CRUMB_W; // comfortable, not the floor
    const bandWidth = Math.max(gutter, naturalWidth, MIN_GLYPH_W);
    // Pin the band's right edge at the pivot; excess width pushes chipLeft left.
    let chipLeft = surfaceLeft - OUT_W - bandWidth;
    if (surfaceLeft > 0 && chipLeft < 0) {
        // Ran out of room on the LEFT: pin ◀ at the page edge (x=0) and keep the
        // comfortable band width, letting it extend RIGHT past the pivot (over the
        // content) instead of crunching/ellipsizing. The content column has room to
        // the right, so right-spill adds no horizontal scrollbar (unlike left-spill,
        // which would push past x=0). The band's right edge is no longer pinned at
        // the pivot in this case — by design.
        chipLeft = 0;
    } else if (surfaceLeft <= 0) {
        // Unmeasured layout (jsdom): keep the old anchor so the full path renders.
        chipLeft = Math.max(0, colLeft - OUT_W);
    }
    const slots = surfaceLeft > 0 ? Math.floor(bandWidth / MIN_GLYPH_W) : crumbCount;
    return { chipLeft, bandWidth, slots };
}

// ── Component ──────────────────────────────────────────────────────────────────

export function BreadcrumbChip(): React.ReactElement | null {
    const ctx = useContext(PreviewContext);
    const chipRef = useRef<HTMLDivElement | null>(null);
    const [geom, setGeom] = useState<ChipGeometry | null>(null);

    const et = ctx?.editTarget;
    const active = !!ctx?.unlockNestingCursor && !!et;

    // ── Geometry effect (Phase 3: content-plane anchor, no scroll listener) ───
    //
    // Fires on editTarget change only. Both the chip's offset-parent
    // (#quarto-content, position:relative) and the surface element live inside
    // #root's scroll container, so a once-computed offset stays correct under
    // scroll — no recompute, no lag.
    useLayoutEffect(() => {
        if (!active) { setGeom(null); return; }

        // --- surface ---
        // Anchor to the actual editing surface (the <textarea>), not the
        // #q2-active-edit-region wrapper: the wrapper spans the full text column
        // (left = colLeft) for every block, so anchoring to it loses the block's
        // indent. The textarea sits at the block's real content left (indented for
        // list/blockquote items), which is the "surface left edge" the crumb row
        // must meet. Fall back to the wrapper when no textarea is mounted.
        const editRegion = ctx?.activeEditRegionRef?.current;
        if (!editRegion) { setGeom(null); return; }
        const surface = editRegion.querySelector('textarea') ?? editRegion;

        // --- host (#quarto-content, the chip's offset-parent) ---
        // Look up directly by id — NOT via surface.offsetParent.
        // Using surface.offsetParent was the prior defect: when #quarto-content
        // had no `position`, the chip's containing block resolved to the
        // viewport ICB (which doesn't move when #root scrolls internally),
        // causing the chip to detach from the surface on scroll.
        const host = document.getElementById('quarto-content');
        if (!host) { setGeom(null); return; }

        const hostRect = host.getBoundingClientRect();
        const sRect = surface.getBoundingClientRect();

        // Coords relative to host (scroll-stable because both share #root scroll)
        const surfaceLeft = sRect.left - hostRect.left;
        const surfaceTop = sRect.top - hostRect.top;

        // Text-column left margin (colLeft): ◀'s right edge pins here, and the crumb
        // band starts here — so only ◀ is in the outer margin, never the crumbs.
        const mainEl = document.querySelector('main#quarto-document-content');
        const colLeft = mainEl
            ? mainEl.getBoundingClientRect().left - hostRect.left
            : surfaceLeft;

        // --- Source crumbs (outermost-first, current = last) ---
        const crumbs = et
            ? buildAncestorPath(ctx?.sourceIndex, et.anchorR0, et.anchorR1)
            : [];

        // --- Layout: pivot-pinned left-spill (G1) ---
        // The band's right edge is pinned at the pivot (surfaceLeft); it is sized to
        // the comfortable naturalWidth (crumbs * CRUMB_W), so a shallow / zero-indent
        // path spills the chip LEFT into the page margin rather than collapsing. ◀
        // rides the chip's left edge; ▶ + the future placeholder sit just right of the
        // pivot (over the content). See computeChipGeometry for the full contract.
        const { chipLeft, bandWidth, slots } = computeChipGeometry(
            surfaceLeft,
            colLeft,
            crumbs.length,
        );
        const displayItems = selectDisplayItems(crumbs, slots);

        // --- Chip top: bottom edge flush at surface top ---
        const chipH = chipRef.current?.getBoundingClientRect().height ?? 0;
        const top = surfaceTop - chipH;

        setGeom({ top, chipLeft, bandWidth, displayItems });
    }, [active, et?.anchorR0, et?.anchorR1, ctx?.activeEditRegionRef, ctx?.sourceIndex]);

    if (!active || !et) return null;

    const crumbs = buildAncestorPath(ctx?.sourceIndex, et.anchorR0, et.anchorR1);
    const platform = detectPlatform();
    const outTip = platform === 'mac' ? 'Out (⌘⌃←)' : 'Out (Alt+Shift+←)';
    const inTip = platform === 'mac' ? 'In (⌘⌃→)' : 'In (Alt+Shift+→)';

    // stopPropagation: the host (#quarto-content) carries delegated pointer
    // handlers (useBlockEditHover); the chip must fully intercept its own pointer
    // events so a chip click is never read as a leaf-reset/click-switch.
    // preventDefault on pointerdown keeps the textarea focused (no blur-commit on
    // a button press). [Real focus/blur + pointer-ordering: verified in P3.5.]
    const eat = (e: React.PointerEvent) => { e.stopPropagation(); e.preventDefault(); };

    // Determine display items for the crumb row.
    // When geom is available, use geom.displayItems (may be ellipsized).
    // When geom is null (first render / not yet measured), fall back to full crumbs
    // at natural widths (unpositioned chip renders for height measurement).
    const displayItems: CrumbDisplayItem[] = geom
        ? geom.displayItems
        : crumbs.map((crumb) => ({ kind: 'crumb' as const, crumb }));

    return (
        <>
            <style>{`
                /* Phase 3: make #quarto-content the chip's offset-parent.
                   This creates a stacking context on the page-columns grid
                   container (spans screen-start → screen-end). Risk is LOW:
                   no position:fixed children inside #quarto-content; the
                   attribution overlay renders outside it; sidebar/TOC/overlays
                   have their own stacking contexts at the page-grid level.
                   The chip's z-index:50 paints above sidebar(z≈1)/main(z=0). */
                #quarto-content { position: relative; }

                .q2-breadcrumb-chip {
                    display: flex;
                    align-items: center;
                    gap: 0;
                    pointer-events: auto;
                    /* No horizontal padding: chipLeft + ◀ width + band width must sum
                       exactly to surfaceLeft so the crumb row's right edge meets the
                       pivot (computeChipGeometry pins it there). Vertical breathing
                       only. The opaque fill (set below) gives the glyphs legibility
                       over occluded content. */
                    padding: 1px 0;
                    /* G13: opaque pill — a very faint cool blue-grey (B highest,
                       G a touch over R). The previous translucent white +
                       backdrop-filter read as murky over occluded content; an opaque
                       fill fully covers what's behind, so the filter is redundant. */
                    background: rgb(243, 247, 250);
                    border-radius: 4px;
                    box-shadow: 0 1px 3px rgba(0,0,0,0.12);
                }
                /* Fixed-width crumb band: [◀.right, surfaceLeft]. Crumbs flex to fill
                   it (expand when few, shrink when many); width is set inline. */
                .q2-breadcrumb-crumbs {
                    display: flex;
                    align-items: center;
                    gap: 0;
                    overflow: hidden;
                }
                .q2-crumb {
                    border: none;
                    background: transparent;
                    font-size: 12px;
                    padding: 1px 3px;
                    cursor: pointer;
                    color: inherit;
                    line-height: 1.4;
                    overflow: hidden;
                    white-space: nowrap;
                    text-overflow: ellipsis;
                    text-align: center;
                    /* Share the band equally; shrink below content width if needed. */
                    flex: 1 1 0;
                    min-width: 0;
                }
                .q2-crumb-current {
                    font-weight: bold;
                    text-decoration: underline;
                }
                .q2-crumb:not(.q2-crumb-current):hover {
                    text-decoration: underline;
                }
                .q2-breadcrumb-out,
                .q2-breadcrumb-in {
                    border: none;
                    background: transparent;
                    font-size: 11px;
                    padding: 1px 4px;
                    cursor: pointer;
                    color: #555;
                    line-height: 1.4;
                    border-radius: 3px;
                    flex-shrink: 0;
                }
                .q2-breadcrumb-out:hover,
                .q2-breadcrumb-in:hover {
                    background: rgba(0,0,0,0.08);
                }
                .q2-crumb-cat-container { color: #4f46e5; }
                .q2-crumb-cat-list      { color: #15803d; }
                .q2-crumb-cat-quote     { color: #b45309; }
                .q2-crumb-cat-leaf-text { color: #0284c7; }
                .q2-crumb-cat-embed     { color: #0f766e; }
                .q2-breadcrumb-future   { opacity: 0.4; }
                .q2-crumb-ellipsis {
                    font-size: 12px;
                    padding: 1px 3px;
                    color: #888;
                    line-height: 1.4;
                    flex: 0 0 auto;
                    user-select: none;
                }
            `}</style>
            {/* Phase 3d: position:absolute — never reflows; paints into outer margin
                (no overflow:hidden on #quarto-content or page-columns ancestors). */}
            <div
                ref={chipRef}
                className="q2-breadcrumb-chip"
                data-testid="q2-breadcrumb-chip"
                role="toolbar"
                aria-label="Nesting breadcrumb"
                onPointerDown={eat}
                onPointerUp={(e) => e.stopPropagation()}
                style={{
                    position: 'absolute',
                    // #quarto-content is a CSS grid (.page-columns). An abspos child
                    // of a grid is contained by its GRID AREA, not the grid box — so
                    // without this it auto-places into the body content column and
                    // `left:0` resolves to the column edge (colLeft), unable to reach
                    // the outer page margin. Spanning screen-start→screen-end makes the
                    // grid area the full page width, so computed left/top (measured vs
                    // #quarto-content) resolve against the full box and margin-spill works.
                    gridColumn: 'screen-start / screen-end',
                    gridRow: '1 / -1',
                    top: geom ? `${geom.top}px` : undefined,
                    // Left edge from computeChipGeometry (pivot-pinned left-spill):
                    // surfaceLeft − ◀ − band, clamped to ≥ 0 at the page edge.
                    left: geom ? `${geom.chipLeft}px` : undefined,
                    zIndex: 50,
                }}
            >
                {/* ◀ out-arrow — fixed width, sits in the left page margin. */}
                <button
                    type="button"
                    className="q2-breadcrumb-out"
                    title={outTip}
                    aria-label={outTip}
                    style={{ minWidth: `${MIN_GLYPH_W}px`, maxWidth: `${MIN_GLYPH_W}px`, flex: '0 0 auto' }}
                    onPointerDown={(e) => e.preventDefault()}
                    onClick={(e) => { e.stopPropagation(); ctx?.requestNestingMove?.('out'); }}
                >◀</button>
                {/* Crumb band — fixed width [◀.right, surfaceLeft]; crumbs flex to fill,
                    so their right edge meets the surface left (the pivot). */}
                <div
                    className="q2-breadcrumb-crumbs"
                    style={geom ? { width: `${geom.bandWidth}px`, flex: '0 0 auto' } : undefined}
                >
                    {displayItems.map((item, idx) => {
                        if (item.kind === 'ellipsis') {
                            return (
                                <span
                                    key={`ellipsis-${idx}`}
                                    className="q2-crumb-ellipsis"
                                    aria-hidden="true"
                                >…</span>
                            );
                        }
                        const c = item.crumb;
                        return (
                            <button
                                key={`${c.r0}-${c.r1}`}
                                type="button"
                                className={[
                                    'q2-crumb',
                                    `q2-crumb-cat-${c.category}`,
                                    c.isCurrent ? 'q2-crumb-current' : '',
                                ].filter(Boolean).join(' ')}
                                title={c.label}
                                aria-label={c.label}
                                aria-current={c.isCurrent ? 'true' : undefined}
                                onPointerDown={(e) => e.preventDefault()}
                                onClick={(e) => { e.stopPropagation(); ctx?.requestNestingSelect?.(c.r0, c.r1); }}
                            >{c.abbrev}</button>
                        );
                    })}
                </div>
                {/* ▶ in-arrow + future-crumb placeholder — just right of the pivot
                    (over the content). The placeholder reserves the successor plan's
                    forward-crumb slot. */}
                <button
                    type="button"
                    className="q2-breadcrumb-in"
                    title={inTip}
                    aria-label={inTip}
                    style={{ flex: '0 0 auto' }}
                    onPointerDown={(e) => e.preventDefault()}
                    onClick={(e) => { e.stopPropagation(); ctx?.requestNestingMove?.('in'); }}
                >▶</button>
                <span className="q2-breadcrumb-future" />
            </div>
        </>
    );
}

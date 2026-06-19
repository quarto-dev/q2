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
 * ## Layout model (gutter-fill + margin-spill, Phase 3b/3c)
 *
 * Crumbs occupy the indent gutter — the horizontal band from the active
 * surface's left edge (the pivot) back toward and past the page text column.
 * Surface-left is the right-anchor of the crumb row; crumbs grow leftward,
 * outermost (◀) furthest left.
 *
 *   page edge  text-col margin (colLeft)     surface-left (pivot)
 *   |  ◀  Dv  |  ❝  •  ¶  |              [ editing surface text … ]
 *
 * Each ancestor crumb is sized to the indent band its container contributes
 * (measured from DOM via getBoundingClientRect). Non-indenting containers
 * (Div, Figure) have near-zero bands and spill left into the outer page margin.
 *
 * Hard stop: leftmost crumb clamped at `#quarto-content`'s left edge (≈ x=0
 * relative to host) so the chip never leaves `#root`'s content box (that would
 * add a horizontal scrollbar). When crumbs don't fit [0, surfaceLeft], they
 * are first compressed to MIN_GLYPH_W, then middle crumbs are ellipsized.
 */

import React, { useContext, useLayoutEffect, useRef, useState } from 'react';
import { PreviewContext } from './PreviewContext';
import { buildAncestorPath, detectPlatform } from './nestingNav';
import type { AncestorCrumb } from './nestingNav';

// ── Constants ──────────────────────────────────────────────────────────────────

/**
 * Minimum legible width (px) for the ◀ button and for sizing how many crumbs
 * can share the gutter before the middle must ellipsize. Most crumb glyphs are a
 * single character (•, ¶, H2, Dv, 1.) at 12px font with ~3px padding each side,
 * so ~16px is the smallest that stays legible. Kept tight so an indented block's
 * immediate parent crumb still fits the gutter alongside the current crumb.
 */
const MIN_GLYPH_W = 16;

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
    /** left edge of the chip (px, relative to #quarto-content) = colLeft − ◀ width.
     *  ◀ sits in the outer margin with its RIGHT edge flush at the text-column left
     *  (colLeft); nothing else enters the margin. */
    chipLeft: number;
    /** Width (px) of the crumb band = the indent gutter [colLeft, surfaceLeft],
     *  clamped to ≥ MIN_GLYPH_W. The crumbs flex to fill it, so their right edge
     *  meets surfaceLeft (the pivot) for indented blocks; for a zero-indent block
     *  the gutter is empty so the band overshoots right into the content (never
     *  into the margin). ▶ sits just to the right of the band. */
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

        // --- Layout: ◀ in the margin (right edge at colLeft); crumbs fill the gutter ---
        // ◀ occupies [colLeft − OUT_W, colLeft] — in the outer margin, flush against
        // the text column. The crumb band fills the indent gutter [colLeft, surfaceLeft]
        // so the crumbs' right edge MEETS the surface left (the pivot); ▶ + the future
        // placeholder sit just to the right of surfaceLeft (over the content). Nothing
        // but ◀ is ever in the margin.
        const OUT_W = MIN_GLYPH_W;
        const chipLeft = Math.max(0, colLeft - OUT_W);
        // Gutter width; clamp to ≥ MIN_GLYPH_W so the current crumb stays visible for
        // a zero-indent block — it then overshoots RIGHT into the content (never into
        // the margin), rather than vanishing.
        const gutter = surfaceLeft - colLeft;
        const bandWidth = Math.max(gutter, MIN_GLYPH_W);
        // How many crumbs fit at a legible width before the middle must ellipsize.
        // When surfaceLeft is non-positive there is no real layout to measure
        // (jsdom returns zero rects) — show the full path rather than collapsing.
        const slots = surfaceLeft > 0 ? Math.floor(bandWidth / MIN_GLYPH_W) : crumbs.length;
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
                    /* No horizontal padding: ◀ width + band width must sum exactly to
                       surfaceLeft so the crumb row meets the pivot. Vertical breathing
                       only. The faint scrim gives the glyphs legibility over occluded
                       content without a heavy pill. */
                    padding: 1px 0;
                    background: rgba(255, 255, 255, 0.85);
                    backdrop-filter: blur(2px);
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
                    // ◀ sits in the margin with its right edge at the text column;
                    // chipLeft = colLeft − ◀ width.
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

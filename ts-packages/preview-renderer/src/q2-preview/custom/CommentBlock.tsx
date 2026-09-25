/**
 * Default comment chrome for q2-preview blocks.
 *
 * Comments are `[>> ...]` editorial-mark spans (class
 * `quarto-edit-comment`) stored inline in the block's own source. This
 * component extracts them before rendering and shows them as a small
 * "bubble" at the block's top-right corner. The block itself renders
 * UNTOUCHED — the bubble (and the block glow) live in one body-level
 * overlay layer, positioned from the block's measured rect, because a
 * wrapper element would break every `parent > child` theme rule
 * (bd-q2wqj24c; see `commentAnchor.tsx` for how the chrome finds the
 * block's element):
 *
 *  - Compact bubble ('show' mode): first comment preview + "+n more";
 *    a comment-less block shows a "+" add affordance on hover.
 *  - Expanded bubble (global 'expand' mode, or any bubble clicked
 *    open): every comment as its own row with author dot (when the
 *    Authors overlay provides attribution), a ✓ resolve button, and an
 *    inline add-comment input at the bottom.
 *  - 'hide' mode strips comments from the text but renders no chrome.
 *
 * Comment text renders RICH (bd-y66gbfs4): the span's inlines go
 * through the normal q2-preview inline renderers via a bubble-scoped
 * registry override (see `CommentSpanContent`) — emphasis, code,
 * quotes, links, and clamped images render for real; nested editorial
 * marks and Notes render an affirmative `[unsupported content]` chip;
 * link clicks route through `routeLinkClick` at the chrome level.
 *
 * The three-way mode arrives via `PreviewContext.commentsMode` from the
 * host toolbar. Adds/resolves round-trip through
 * `usePreviewEdit().commitSubtreeEdit` (no-ops where `PreviewContext`
 * is absent). Blocks without an inline slot (code blocks, incl.
 * mermaid) get wrapped in a `CONTAINER_CLASS` Div holding the comment
 * paragraphs; the Div owns the thread's single bubble and its children
 * render chrome-free.
 *
 * Overlapping bubbles are kept apart by a tiny force layout (see the
 * "force layout" section below).
 *
 * Registered as the `Block` entry in `registry.ts`. Delegates actual
 * block rendering to the `dispatchers.tsx` `Block` dispatcher, so edit
 * substitution and attribution wrapping are untouched. A user
 * render-components override of `Block` still wins via
 * `mergedPreviewRegistry` (and receives the raw dispatcher as
 * `__Q2_PREVIEW_RENDERER__.Block`, exactly as before).
 */
import React from 'react';
import { createPortal } from 'react-dom';
import { AttributionLookupContext, Node as AstNode, RegistryContext } from '../../framework';
import type {
    BlockNode,
    DivBlock,
    HeaderBlock,
    InlineNode,
    NodeArgs,
    ParaBlock,
    PlainBlock,
    SpanInline,
} from '../../framework';
import { Block as B } from '../dispatchers';
import { PreviewContext } from '../PreviewContext';
import type { CommentsMode } from '../PreviewContext';
import { usePreviewEdit } from '../usePreviewEdit';
import { routeLinkClick } from '../../utils/iframeLinkHandlers';
import { CommentAnchorContext, PlainHostContext } from '../commentAnchor';
import type { CommentAnchorTarget } from '../commentAnchor';

// Shared palette bits.
const CHROME_BLUE = '#4a7ba7';
const DIVIDER = '1px solid rgba(74, 123, 167, 0.3)';
const GLOW = '0 0 8px 2px rgba(140, 190, 240, 0.6)';

function isComment(inline: InlineNode): boolean {
    if (inline.t === 'Span' && 'c' in inline) {
        const classes = (inline as SpanInline).c[0][1];
        return classes.includes('quarto-edit-comment');
    }
    return false;
}

// A block that can't hold an inline comment span (a code block) gets
// wrapped in a Div with this class; the comment lives as a `[>> ...]`
// paragraph inside the Div. The Div owns the one bubble for the whole
// thread; its children render chrome-free (InsideCommentContainer).
const CONTAINER_CLASS = 'quarto-edit-comment-container';

function isCommentContainer(block: BlockNode): boolean {
    return (
        block.t === 'Div' &&
        ((block as DivBlock).c[0][1] ?? []).includes(CONTAINER_CLASS)
    );
}

const InsideCommentContainer = React.createContext(false);

/**
 * The mutable inline array where a block's comment spans live, or null
 * when the block type has no inline slot. (Casts because the BlockNode
 * union includes `UnknownBlock { t: string }`, which defeats
 * discriminant narrowing.)
 */
function inlineSlot(block: BlockNode): InlineNode[] | null {
    if (block.t === 'Para' || block.t === 'Plain') {
        return (block as ParaBlock | PlainBlock).c;
    }
    if (block.t === 'Header') return (block as HeaderBlock).c[2];
    return null;
}

// Light blue placeholder tint for the comment input — ::placeholder
// isn't reachable from inline styles, so inject one tiny rule per
// document (idempotent).
(() => {
    if (typeof document === 'undefined') return;
    if (document.head.querySelector('style[data-q2-comment-styles]')) return;
    const tag = document.createElement('style');
    tag.setAttribute('data-q2-comment-styles', '1');
    tag.textContent =
        '.q2-comment-input::placeholder { color: #a9c7e8; opacity: 1; }\n' +
        // Image containment (bd-y66gbfs4): an unconstrained <img> would
        // widen the shrink-to-fit bubble to its intrinsic size. `100%`
        // resolves against the bubble's inner max-width'd containers.
        '.q2-comment-bubble img { max-width: 100%; max-height: 2.5em; object-fit: contain; }';
    document.head.appendChild(tag);
})();

/**
 * True when the rendered block and its resolved source node are the
 * same simple kind we know how to modify + serialize safely. A
 * mismatch means the rendered block is a transform product — a figure
 * caption resolving to the whole `Figure`, a definition-list item
 * resolving to the `DefinitionList` — whose source form the qmd writer
 * would rewrite lossily (figures became `::: {#fig-..}` divs,
 * `::: {.definition-list}` sugar became bare `term\n:   def` syntax).
 * Refuse to touch those.
 */
function sameCommentableKind(rendered: BlockNode, source: BlockNode): boolean {
    const paraish = (t: string) => t === 'Para' || t === 'Plain';
    if (paraish(rendered.t) && paraish(source.t)) {
        // An implicit figure is a paragraph holding just an image —
        // appending an inline span next to the image de-figures it.
        const inlines = (source as ParaBlock | PlainBlock).c;
        const solid = inlines.filter((n) => n.t !== 'Space' && n.t !== 'SoftBreak');
        return !(solid.length === 1 && solid[0].t === 'Image');
    }
    if (rendered.t === 'Header' && source.t === 'Header') return true;
    if (rendered.t === 'CodeBlock' && source.t === 'CodeBlock') return true;
    if (isCommentContainer(rendered) && isCommentContainer(source)) return true;
    return false;
}

// ---------------------------------------------------------------------
// Rich bubble content (bd-y66gbfs4). The bubble renders the comment
// span's inlines through the normal q2-preview inline renderers via the
// framework's <Node> dispatch, with two deliberate deviations installed
// through a bubble-scoped registry override:
//
//  - Editorial-mark spans and Notes render an affirmative
//    `[unsupported content]` chip instead of their content. In the wire
//    format ALL four editorial marks are `t: 'Span'` distinguished only
//    by class, so the interception is class-aware inside a Span
//    override (a tag-keyed entry can't see them). Nesting is handled
//    for free: the provider wraps the whole bubble subtree, so
//    recursion through any renderer re-enters the override.
//  - `setLocalAst` is a no-op: the bubble displays the comment; all
//    comment mutation goes through addComment / resolveCommentAtIndex
//    on the resolved source node.
//
// Plan: claude-notes/plans/2026-08-26-rich-comment-bubbles.md

const EDITORIAL_MARK_CLASSES = new Map<string, string>([
    ['quarto-edit-comment', 'nested comment'],
    ['quarto-insert', 'insertion mark'],
    ['quarto-delete', 'deletion mark'],
    ['quarto-highlight', 'highlight mark'],
]);

// Generic text on purpose — the chip must stay narrow in a 140px
// bubble; the tooltip carries the kind.
const UnsupportedChip = ({ kind }: { kind: string }) => (
    <span
        title={`unsupported in comment bubbles: ${kind}`}
        style={{
            fontFamily: 'monospace',
            fontSize: '0.9em',
            fontStyle: 'normal',
            opacity: 0.65,
        }}
    >
        [unsupported content]
    </span>
);

const NOOP_SET_LOCAL_AST = () => {};

/**
 * Renders one comment span's content inlines for display in the
 * bubble, under the bubble-scoped registry override described above.
 */
const CommentSpanContent = ({
    span,
    onNavigateToDocument,
}: {
    span: InlineNode;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}) => {
    const outer = React.useContext(RegistryContext);
    const bubbleValue = React.useMemo(() => {
        const OuterSpan = outer.registry['Span'];
        const BubbleSpan = (props: NodeArgs<InlineNode>) => {
            const classes = (props.node as SpanInline).c[0][1] ?? [];
            const marked = classes.find((c) => EDITORIAL_MARK_CLASSES.has(c));
            if (marked !== undefined) {
                return <UnsupportedChip kind={EDITORIAL_MARK_CLASSES.get(marked)!} />;
            }
            return OuterSpan ? <OuterSpan {...props} /> : null;
        };
        const registry = {
            ...outer.registry,
            Span: BubbleSpan,
            // Block content has no place in the chip-sized bubble.
            Note: () => <UnsupportedChip kind="footnote" />,
        };
        return { registry, sourceInfoPool: outer.sourceInfoPool };
    }, [outer.registry, outer.sourceInfoPool]);

    return (
        <RegistryContext.Provider value={bubbleValue}>
            {(span as SpanInline).c[1].map((inline, i) => (
                <AstNode
                    key={i}
                    node={inline}
                    onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={NOOP_SET_LOCAL_AST}
                />
            ))}
        </RegistryContext.Provider>
    );
};

export const CommentBlock = (args: NodeArgs<BlockNode>) => {
    const edit = usePreviewEdit();
    const insideContainer = React.useContext(InsideCommentContainer);
    const mode: CommentsMode =
        React.useContext(PreviewContext)?.commentsMode ?? 'show';
    const plainHost = React.useContext(PlainHostContext);
    const { node: block, onNavigateToDocument, setLocalAst } = args;

    const passthrough = (
        <B node={block} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst} />
    );

    // Children of a comment container defer to the container's bubble.
    if (insideContainer) return passthrough;

    // Extract this block's comment spans and build a stripped copy for
    // rendering. Blocks without comments render as-is (no clone).
    let comments: InlineNode[] = [];
    let newBlock = block;
    const slot = inlineSlot(block);
    if (slot) {
        comments = slot.filter(isComment);
        if (comments.length > 0) {
            const clone = structuredClone(block);
            const cloneSlot = inlineSlot(clone)!;
            cloneSlot.splice(0, cloneSlot.length, ...slot.filter((n) => !isComment(n)));
            newBlock = clone;
        }
    } else if (isCommentContainer(block)) {
        // Collect comment spans from the container's paragraphs and
        // strip them (dropping paragraphs that were nothing but
        // comments) so only the wrapped content renders.
        const clone = structuredClone(block) as DivBlock;
        const kept: BlockNode[] = [];
        for (const child of clone.c[1]) {
            const childSlot = inlineSlot(child);
            if (childSlot) {
                comments.push(...childSlot.filter(isComment));
                childSlot.splice(0, childSlot.length, ...childSlot.filter((n) => !isComment(n)));
                if (childSlot.length === 0) continue;
            }
            kept.push(child);
        }
        clone.c[1] = kept;
        newBlock = clone;
    }

    // The chrome applies to every block type that can store a comment:
    // Para, Plain (list items), and Header hold an inline comment span;
    // commenting on a CodeBlock (incl. mermaid) wraps it in a comment
    // container Div, which then owns the thread's single bubble. That
    // covers essentially everything hoverable/editable; other block
    // types (lists as a whole, tables, ...) render plain.
    const canHoldComment =
        slot !== null || block.t === 'CodeBlock' || isCommentContainer(block);
    if (!canHoldComment) return passthrough;

    // A Plain renders a fragment — its inlines land directly in the
    // parent element — so its bubble anchors to that element, provided
    // by the component that owns it (`PlainHost`: a tight list's <li>, a
    // definition's <dd>). With no host in scope there is nothing to
    // anchor to: render as-is, comment spans visible in the text, rather
    // than stripping the comment and showing no bubble (bd-q2wqj24c).
    if (block.t === 'Plain' && !plainHost) return passthrough;

    // Commenting on figures, definition lists, and table cells is
    // BUSTED right now: committing a Figure or DefinitionList source
    // node re-serializes it lossily (broken syntax), and table cells
    // are Opaque (writes never land). Hide the bubble there entirely —
    // addComment/resolveCommentAtIndex refuse the same cases as a
    // backstop. Blocks that already carry comments keep their bubble
    // for read-only display.
    if (comments.length === 0) {
        // `resolveSource` is a pluggable context member — treat a
        // malformed entry (no sourceNode) like an unresolvable block
        // rather than crashing the render (bd-ddaqjb91).
        const resolved = edit.resolveSource(block);
        if (
            !resolved ||
            !resolved.sourceNode ||
            resolved.reachabilityClass === 'Opaque' ||
            !sameCommentableKind(block, resolved.sourceNode)
        ) {
            return passthrough;
        }
    }

    const inner = (
        <B node={newBlock} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst} />
    );
    const content = isCommentContainer(block) ? (
        <InsideCommentContainer.Provider value={true}>
            {inner}
        </InsideCommentContainer.Provider>
    ) : (
        inner
    );
    // 'hide' mode: comments stay stripped from the text, but no chrome
    // renders at all.
    if (mode === 'hide') return content;
    return (
        <CommentWrapper
            comments={comments}
            block={block}
            rendered={newBlock}
            edit={edit}
            mode={mode}
            onNavigateToDocument={onNavigateToDocument}
        >
            {content}
        </CommentWrapper>
    );
};

type EditHandle = ReturnType<typeof usePreviewEdit>;

// Only one self-expanded bubble at a time: expanding one collapses the
// previously expanded one via this module-level latch.
let collapseExpandedBubble: (() => void) | null = null;

// ---------------------------------------------------------------------
// Overlay layer (bd-q2wqj24c). Every bubble is portalled into ONE
// body-level, zero-size, absolutely positioned layer and placed from its
// block's measured rect in the layer's own coordinates (block rect −
// layer rect, both measured in the same frame). Measuring the layer
// rather than assuming document coordinates keeps the math right
// whatever the body's margin or `position`, scrolled or not, and the
// same code serves documents and reveal decks (the layer sits outside
// the deck's `.slides` transform, so bubbles need no counter-scale).
// The block itself is rendered untouched — a wrapper element would
// break every `parent > child` theme rule.
const LAYER_ATTR = 'data-q2-comment-layer';
let layerEl: HTMLElement | null = null;

function getCommentLayer(): HTMLElement {
    if (layerEl && layerEl.isConnected) return layerEl;
    const existing = document.body.querySelector<HTMLElement>(`[${LAYER_ATTR}]`);
    if (existing) {
        layerEl = existing;
        return existing;
    }
    const el = document.createElement('div');
    el.setAttribute(LAYER_ATTR, '');
    Object.assign(el.style, {
        position: 'absolute',
        top: '0',
        left: '0',
        width: '0',
        height: '0',
        overflow: 'visible',
        pointerEvents: 'none',
    });
    document.body.appendChild(el);
    layerEl = el;
    return el;
}

// Natural spot of the chrome relative to its block: 11px above the top
// edge, right-aligned 10px past the right edge (the chrome translates
// by -100% horizontally so no width measurement is needed).
const BUBBLE_TOP_OFFSET = 11;
const BUBBLE_RIGHT_OFFSET = 10;
// Bubbles on reveal slides read small next to slide-sized type (the deck
// itself is scaled to fit; the layer is not), so deck chrome is scaled
// up by this factor. Tune to taste; only applies inside decks.
const DECK_BUBBLE_SCALE = 1.2;

// ---------------------------------------------------------------------
// Delegated hover. Without a wrapper there is no element whose
// mousemove/mouseleave can drive the per-block hover, so one document
// listener resolves the pointer to the block under it: walk the
// target's ancestors against an index of registered anchors (block
// host elements) and chrome containers — the first hit is the deepest,
// which is the right block for nested anchors (a tight `<li>` holding a
// loose sub-list). The hovered record gets the pointer; the previously
// hovered one is told the pointer left.
type HoverRecord = {
    getAnchor: () => Element | null;
    /** The anchor as last indexed (anchors can attach late — see PlainHost). */
    anchorEl: Element | null;
    /** The chrome container (portalled into the layer), while visible. */
    chromeEl: Element | null;
    /** The `.q2-comment-bubble` inside the chrome, while visible. */
    bubbleEl: Element | null;
    onPointer: (p: { x: number; y: number; hovered: boolean; inBubble: boolean }) => void;
    onLeave: () => void;
};
const hoverRecords = new Set<HoverRecord>();
const hoverIndex = new WeakMap<Element, HoverRecord>();
let hoveredRecord: HoverRecord | null = null;

function indexAnchor(r: HoverRecord) {
    const a = r.getAnchor();
    if (a === r.anchorEl) return;
    if (r.anchorEl) hoverIndex.delete(r.anchorEl);
    r.anchorEl = a;
    if (a) hoverIndex.set(a, r);
}

function setRecordChrome(r: HoverRecord, chrome: Element | null, bubble: Element | null) {
    if (r.chromeEl && r.chromeEl !== chrome) hoverIndex.delete(r.chromeEl);
    r.chromeEl = chrome;
    r.bubbleEl = bubble;
    if (chrome) hoverIndex.set(chrome, r);
}

function onDocumentMouseMove(e: MouseEvent) {
    // Late-bound anchors: a Plain's host <li> attaches its ref after the
    // Plain's own effects ran, so resolve any still-missing anchor now.
    for (const r of hoverRecords) if (!r.anchorEl) indexAnchor(r);
    let hit: HoverRecord | null = null;
    for (let el = e.target as Element | null; el; el = el.parentElement) {
        const r = hoverIndex.get(el);
        if (r) {
            hit = r;
            break;
        }
    }
    if (hoveredRecord && hoveredRecord !== hit) hoveredRecord.onLeave();
    hoveredRecord = hit;
    if (!hit) return;
    const inBubble = !!hit.bubbleEl?.contains(e.target as Node);
    // Only the RIGHT half of the block counts as hover (the bubble lives
    // at the right edge) — mousing across the left half while reading
    // doesn't reveal chrome or reshuffle the bubble layout. Moves over
    // the bubble itself always count.
    const rect = hit.anchorEl?.getBoundingClientRect();
    const hovered = inBubble || (!!rect && e.clientX >= rect.left + rect.width / 2);
    hit.onPointer({ x: e.clientX, y: e.clientY, hovered, inBubble });
}

function onDocumentMouseLeave() {
    if (!hoveredRecord) return;
    hoveredRecord.onLeave();
    hoveredRecord = null;
}

function addHoverRecord(r: HoverRecord) {
    if (hoverRecords.size === 0) {
        document.addEventListener('mousemove', onDocumentMouseMove);
        document.documentElement.addEventListener('mouseleave', onDocumentMouseLeave);
    }
    hoverRecords.add(r);
    indexAnchor(r);
}

function removeHoverRecord(r: HoverRecord) {
    hoverRecords.delete(r);
    if (r.anchorEl) hoverIndex.delete(r.anchorEl);
    if (r.chromeEl) hoverIndex.delete(r.chromeEl);
    if (hoveredRecord === r) hoveredRecord = null;
    if (hoverRecords.size === 0) {
        document.removeEventListener('mousemove', onDocumentMouseMove);
        document.documentElement.removeEventListener('mouseleave', onDocumentMouseLeave);
    }
}

// ---------------------------------------------------------------------
// Tiny force layout. Visible bubbles register here; a batched rAF pass
// places each one from its block's rect and keeps them from
// overlapping, under these rules:
//  - the hovered bubble is pinned at its natural spot (nudged below the
//    viewport top if needed) and everything else moves around it;
//  - pushes are directional in DOCUMENT order (earlier bubbles only get
//    pushed up, later ones only down), so bubbles never reorder;
//  - idle bubbles keep their displacement between passes (push-only),
//    but drift back toward natural whenever free space allows;
//  - a comments-mode switch does a full reset solve from naturals.
const BUBBLE_GAP = 4;
type BubbleEntry = {
    /** The chrome container in the layer. */
    el: HTMLElement | null;
    /** The block's host element (null while it has not attached / while editing). */
    getAnchor: () => Element | null;
    /** The block-glow overlay element, while mounted. */
    glow: HTMLElement | null;
    /** Nudge in viewport px. */
    nudge: number;
    /** Block currently hovered — its bubble is pinned at its natural spot. */
    hovered: boolean;
    setNudge: (y: number) => void;
    /** The anchor currently under the ResizeObserver. */
    observed: Element | null;
    /** The anchor whose typography / deck-ness the chrome last adopted. */
    styledFrom: Element | null;
    setInDeck: (inDeck: boolean) => void;
};

if (typeof window !== 'undefined') {
    // Deck lifecycle signal (ready / resize / slidechanged + a slow
    // tick while a deck is live): geometry may have changed wholesale
    // — hidden sections never unmount, so slide switches don't
    // re-register anything. Always reset-solve.
    window.addEventListener('q2-reveal-scale', () => {
        scheduleBubbleRelayout(true);
    });
}
const bubbleEntries = new Set<BubbleEntry>();
let bubbleRelayoutScheduled = false;
// When set, the next pass solves from NATURAL positions (full reset)
// instead of from current nudges (push-only). OR-ed across schedule
// calls in the same frame.
let bubbleRelayoutReset = false;

// Geometry freshness (D6 of the plan): a pass runs whenever an anchor or
// the body changes size, on top of the explicit triggers (register,
// hover start, mode switch, image load, deck signal). Content growth
// above a block that does not resize the block moves it; the body
// resize catches that one frame later. Absent in jsdom.
let anchorObserver: ResizeObserver | null | undefined;
let bodyObserved = false;

/** Built on first use — no work at import time. */
function getAnchorObserver(): ResizeObserver | null {
    if (anchorObserver === undefined) {
        anchorObserver =
            typeof ResizeObserver !== 'undefined'
                ? new ResizeObserver(() => scheduleBubbleRelayout())
                : null;
    }
    return anchorObserver;
}

function observeAnchor(e: BubbleEntry, anchor: Element | null) {
    const anchorObserver = getAnchorObserver();
    if (!anchorObserver || e.observed === anchor) return;
    if (e.observed) anchorObserver.unobserve(e.observed);
    e.observed = anchor;
    if (anchor) anchorObserver.observe(anchor);
    if (!bodyObserved) {
        anchorObserver.observe(document.body);
        bodyObserved = true;
    }
}

/**
 * Write the chrome's (and glow's) position for one entry from the
 * block's rect, in layer coordinates. Direct style writes on purpose:
 * this is the DOM-measure-and-write step of a layout pass, and it has
 * to land before the chrome's own rect is measured for overlap solving.
 * React never sets `top`/`left`/`visibility` on these elements, so the
 * writes are not fought over.
 */
function placeEntry(e: BubbleEntry, anchor: Element, anchorRect: DOMRect, layerRect: DOMRect) {
    if (e.el) {
        e.el.style.top = `${anchorRect.top - layerRect.top - BUBBLE_TOP_OFFSET}px`;
        e.el.style.left = `${anchorRect.right - layerRect.left + BUBBLE_RIGHT_OFFSET}px`;
        e.el.style.visibility = '';
        // The chrome sits in the body-level layer, so it no longer
        // inherits the block's typography (a reveal deck sets its font
        // on `.reveal`, not on body). Adopt the block's font family, as
        // the in-tree chrome used to — re-read every pass, not cached:
        // at mount the theme stylesheet may not have applied yet (a
        // deck block measured `Times` on its first placement).
        const fontFamily = getComputedStyle(anchor).fontFamily;
        if (e.el.style.fontFamily !== fontFamily) e.el.style.fontFamily = fontFamily;
        if (e.styledFrom !== anchor) {
            e.styledFrom = anchor;
            e.setInDeck(anchor.closest('.reveal') !== null);
        }
    }
    if (e.glow) {
        e.glow.style.top = `${anchorRect.top - layerRect.top}px`;
        e.glow.style.left = `${anchorRect.left - layerRect.left}px`;
        e.glow.style.width = `${anchorRect.width}px`;
        e.glow.style.height = `${anchorRect.height}px`;
    }
}

/** Place one entry right now (mount time, before paint); hidden until it has an anchor. */
function placeEntryNow(e: BubbleEntry) {
    const anchor = e.getAnchor();
    if (!anchor) {
        if (e.el) e.el.style.visibility = 'hidden';
        return;
    }
    placeEntry(e, anchor, anchor.getBoundingClientRect(), getCommentLayer().getBoundingClientRect());
}

function scheduleBubbleRelayout(reset = false) {
    if (reset) bubbleRelayoutReset = true;
    if (bubbleRelayoutScheduled) return;
    bubbleRelayoutScheduled = true;
    requestAnimationFrame(() => {
        bubbleRelayoutScheduled = false;
        const resetPass = bubbleRelayoutReset;
        bubbleRelayoutReset = false;
        if (!layerEl || !layerEl.isConnected) return;
        const layerRect = layerEl.getBoundingClientRect();

        // Collect the solvable bubbles. Everything below runs in
        // viewport px, for documents and decks alike.
        type Item = {
            e: BubbleEntry;
            anchor: Element;
            top: number;
            height: number;
            left: number;
            right: number;
            cur: number;
            clamp: (y: number) => number;
            pinned: boolean;
        };
        const items: Item[] = [];
        for (const e of bubbleEntries) {
            const el = e.el;
            if (!el) continue;
            const anchor = e.getAnchor();
            observeAnchor(e, anchor);
            // No host element (the block is being edited, or its host
            // never registered): nothing to anchor to, so nothing to show.
            if (!anchor) {
                el.style.visibility = 'hidden';
                continue;
            }
            // SLIDES: only bubbles whose block is on the CURRENT slide
            // participate. Zero-size / visibility checks are NOT
            // enough — reveal keeps nearby slides mounted for
            // preloading (viewDistance) in states that still measure
            // real rects, and their ghost bubbles shove the visible
            // slide's bubbles around. `.present` is reveal's own
            // marker for the active slide (and the active child of a
            // vertical stack). Scoped to `.reveal` so document
            // <section>s are unaffected.
            const section = anchor.closest('.reveal section');
            if (section && !section.classList.contains('present')) {
                el.style.visibility = 'hidden';
                continue;
            }
            // Generic visibility gate (e.g. undisclosed fragments).
            const cv = (anchor as { checkVisibility?: (o?: object) => boolean }).checkVisibility;
            if (cv && !cv.call(anchor, { checkVisibilityCSS: true, visibilityProperty: true })) {
                el.style.visibility = 'hidden';
                continue;
            }
            const anchorRect = anchor.getBoundingClientRect();
            if (anchorRect.width === 0) {
                el.style.visibility = 'hidden';
                continue;
            }
            placeEntry(e, anchor, anchorRect, layerRect);
            const rect = el.getBoundingClientRect();
            if (rect.width === 0) continue;
            // Natural anchor: the chrome's top sits BUBBLE_TOP_OFFSET above
            // the block's top (keep in sync with placeEntry). Never read
            // back from our own transform: that round-trip proved fragile.
            const top = anchorRect.top - BUBBLE_TOP_OFFSET;
            // HOVER PIN: the hovered bubble sits at its natural
            // position (same spot every time, overlapping its block,
            // ready to click) — except it may never sit above the top
            // of the PAGE (a tall expanded bubble on the first block
            // would otherwise be unreachable); its pin shifts down
            // just enough. Going above the viewport top when scrolled
            // is fine. Idle bubbles float freely.
            const TOP_MARGIN = 8;
            const pageTop = TOP_MARGIN - window.scrollY;
            const pinnedTop = Math.max(top, pageTop);
            const clamp = e.hovered ? () => pinnedTop : (y: number) => y;
            // Idle bubbles START from their current (already-nudged)
            // position — a relayout only ever PUSHES them further,
            // never pulls them back (the settle phase below handles
            // drifting home). A reset pass starts from naturals.
            items.push({
                e,
                anchor,
                top,
                height: rect.height,
                left: rect.left,
                right: rect.right,
                cur: e.hovered ? pinnedTop : resetPass ? top : top + e.nudge,
                clamp,
                pinned: e.hovered,
            });
        }
        // DOCUMENT order of the BLOCKS, not visual order: pushes are
        // directional relative to it (earlier-in-document bubbles may
        // only be pushed UP, later ones only DOWN), so document order
        // can never be visually inverted by the layout.
        items.sort((a, b) =>
            a.anchor.compareDocumentPosition(b.anchor) & Node.DOCUMENT_POSITION_FOLLOWING
                ? -1
                : 1,
        );
        const overlapsH = (
            a: { left: number; right: number },
            b: { left: number; right: number },
        ) => a.left < b.right && b.left < a.right;
        // Iterative relaxation: each overlapping pair splits the push —
        // the earlier-in-document bubble moves up, the later one down —
        // clamped to the hover pin. When one side hits its clamp, later
        // rounds shift the remaining overlap onto the other side.
        for (let iter = 0; iter < 40; iter++) {
            let moved = false;
            for (let i = 0; i < items.length; i++) {
                for (let j = i + 1; j < items.length; j++) {
                    const a = items[i];
                    const b = items[j];
                    if (!overlapsH(a, b)) continue;
                    const overlap = a.cur + a.height + BUBBLE_GAP - b.cur;
                    if (overlap > 0) {
                        const aBefore = a.cur;
                        const bBefore = b.cur;
                        a.cur = a.clamp(a.cur - overlap / 2);
                        b.cur = b.clamp(b.cur + overlap / 2);
                        // Whatever the clamps refused, try shoving onto
                        // either side (up first, then down).
                        let remaining = a.cur + a.height + BUBBLE_GAP - b.cur;
                        if (remaining > 0) {
                            a.cur = a.clamp(a.cur - remaining);
                            remaining = a.cur + a.height + BUBBLE_GAP - b.cur;
                            if (remaining > 0) b.cur = b.clamp(b.cur + remaining);
                        }
                        if (a.cur !== aBefore || b.cur !== bBefore) moved = true;
                    }
                }
            }
            if (!moved) break;
        }
        // Settle: bubbles with free space drift back toward their
        // natural position — as far as they can WITHOUT pushing
        // anything (each move only respects neighbors where they
        // currently are; document order is preserved by keeping
        // earlier-in-document neighbors above / later ones below).
        for (let iter = 0; iter < 10; iter++) {
            let moved = false;
            for (let i = 0; i < items.length; i++) {
                const x = items[i];
                if (x.pinned) continue;
                let lo = -Infinity;
                let hi = Infinity;
                for (let j = 0; j < items.length; j++) {
                    if (j === i) continue;
                    const o = items[j];
                    if (!overlapsH(x, o)) continue;
                    if (j < i) lo = Math.max(lo, o.cur + o.height + BUBBLE_GAP);
                    else hi = Math.min(hi, o.cur - x.height - BUBBLE_GAP);
                }
                if (lo > hi) continue; // boxed in — stay put
                const desired = Math.min(Math.max(x.top, lo), hi);
                if (Math.abs(desired - x.cur) > 0.5) {
                    x.cur = desired;
                    moved = true;
                }
            }
            if (!moved) break;
        }
        // Hard no-reorder guarantee: if clamp interactions left any
        // h-overlapping pair visually inverted (later-in-document
        // bubble above an earlier one), push the later bubble DOWN to
        // clear it — the one direction the rule always allows. Skips
        // pinned bubbles (they never move).
        for (let i = 0; i < items.length; i++) {
            for (let j = i + 1; j < items.length; j++) {
                const a = items[i];
                const b = items[j];
                if (!overlapsH(a, b) || b.pinned) continue;
                if (b.cur < a.cur) {
                    b.cur = a.cur + a.height + BUBBLE_GAP;
                }
            }
        }
        for (const it of items) {
            const nudge = Math.round(it.cur - it.top);
            if (nudge !== it.e.nudge) {
                it.e.nudge = nudge;
                it.e.setNudge(nudge);
            }
        }
    });
}

const CommentWrapper = ({
    children,
    comments,
    block,
    rendered,
    edit,
    mode,
    onNavigateToDocument,
}: {
    children: React.ReactNode;
    comments: InlineNode[];
    /** The source block (comment spans included) — what add/resolve commit against. */
    block: BlockNode;
    /** The block as rendered (comments stripped) — the node whose host element anchors the chrome. */
    rendered: BlockNode;
    edit: EditHandle;
    mode: CommentsMode;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}) => {
    const [commentText, setCommentText] = React.useState('');
    const previewCtx = React.useContext(PreviewContext);

    // The block's host element (bd-q2wqj24c): registered by the block
    // component through `CommentAnchorContext` (identity-matched on the
    // rendered node), or — for a Plain, which has no element of its own —
    // the `PlainHost` element it renders into. Null while the block is
    // being edited (the edit surface replaces the component) or when
    // nothing registered; the layout pass hides the chrome then.
    const anchorRef = React.useRef<Element | null>(null);
    const plainHost = React.useContext(PlainHostContext);
    const anchorTarget = React.useMemo<CommentAnchorTarget>(
        () => ({ node: rendered, register: (el) => { anchorRef.current = el; } }),
        [rendered],
    );
    const getAnchorRef = React.useRef<() => Element | null>(() => null);
    getAnchorRef.current = () =>
        anchorRef.current ?? (block.t === 'Plain' ? plainHost?.current ?? null : null);

    /**
     * Route an `<a>` click inside the bubble through the preview's
     * link policy (bd-y66gbfs4). The chrome deliberately
     * stopPropagation()s clicks (they must not reach the click-to-edit
     * delegate), which also keeps them from the delegated body link
     * listener — so the bubble routes its own link clicks through the
     * same extracted logic. Returns true when the click hit a link
     * (routed or swallowed) so callers skip the bubble's own
     * expand/open-input behavior.
     *
     * Simplifications vs. PreviewRoot's wiring (agreed for v1): no
     * `scrollToAnchor` host hook — same-document fragments use a plain
     * smooth `scrollIntoView`; no `projectFilePaths` — artifact hrefs
     * fall back to the `.qmd` candidate.
     */
    const handleBubbleLinkClick = (e: React.MouseEvent): boolean => {
        const anchor = (e.target as Element | null)?.closest?.('a');
        if (!anchor) return false;
        const scrollToFragment = (frag: string) => {
            document.getElementById(frag)?.scrollIntoView({ behavior: 'smooth' });
        };
        const handled = routeLinkClick(e.nativeEvent, {
            currentFilePath: previewCtx?.currentFilePath ?? '',
            onQmdLinkClick: (arg) => {
                if ('path' in arg) {
                    if (arg.path === previewCtx?.currentFilePath) {
                        if (arg.anchor) scrollToFragment(arg.anchor);
                    } else {
                        onNavigateToDocument?.(arg.path, arg.anchor);
                    }
                } else {
                    scrollToFragment(arg.anchor);
                }
            },
        });
        // An unroutable href must still never navigate the preview
        // iframe from inside a bubble — swallow it.
        if (!handled) e.preventDefault();
        return true;
    };

    // Clicking a compact bubble expands it in place (with the inline
    // add-comment input open at its bottom).
    const [selfExpanded, setSelfExpanded] = React.useState(false);
    const [showInlineInput, setShowInlineInput] = React.useState(false);
    const inlineInputRef = React.useRef<HTMLTextAreaElement>(null);
    const [isHovered, setIsHovered] = React.useState(false);
    // Hovering the bubble itself glows the block (mirror of the
    // block-hover → bubble-glow effect). Derived from the DELEGATED
    // document mousemove (pointer containment in the bubble), never from
    // the bubble's own enter/leave (bd-bpt089zw): a resolve re-renders the
    // bubble under a stationary pointer, the browser re-evaluates :hover
    // after layout without boundary events, and the next move's mouseout
    // comes from the NEW hovered node — so a bubble onMouseLeave would
    // never fire and the glow would stick. `bubbleHoveredRef` mirrors the
    // state for the size-change re-check below; `lastPointerRef` is the
    // last pointer position seen over this block or its bubble (viewport
    // px), null once the pointer has left both.
    const [bubbleHovered, setBubbleHovered] = React.useState(false);
    const bubbleHoveredRef = React.useRef(false);
    const lastPointerRef = React.useRef<{ x: number; y: number } | null>(null);
    const updateBubbleHovered = (hovered: boolean) => {
        if (bubbleHoveredRef.current === hovered) return;
        bubbleHoveredRef.current = hovered;
        setBubbleHovered(hovered);
    };
    // Global 'expand' mode expands every commented bubble; a click
    // self-expands one bubble in any mode.
    const expanded = (mode === 'expand' && comments.length > 0) || selfExpanded;
    // Force-layout nudge (translateY) keeping this bubble clear of its
    // neighbors; nudgeRef mirrors it for the module-level relayout pass.
    const [nudge, setNudge] = React.useState(0);
    const nudgeRef = React.useRef(0);
    // Whether the block sits on a reveal slide (set by the layout pass
    // from the anchor); deck chrome is scaled up — see DECK_BUBBLE_SCALE.
    const [inDeck, setInDeck] = React.useState(false);
    const chromeRef = React.useRef<HTMLDivElement>(null);
    const glowRef = React.useRef<HTMLDivElement>(null);
    // Mirror of isHovered for the registry (re-registrations read it),
    // plus the live entry so hover changes can update it in place.
    const isHoveredRef = React.useRef(false);
    const entryRef = React.useRef<BubbleEntry | null>(null);
    const bubbleRef = React.useRef<HTMLDivElement>(null);

    // Per-comment authorship, resolved from the comment span's source
    // pool id (`s`). The lookup is only populated when the host provides
    // attribution (Authors overlay on); otherwise rows render without
    // an author dot.
    const attributionLookup = React.useContext(AttributionLookupContext);
    const commentAuthor = (span: InlineNode) => {
        if (!attributionLookup) return null;
        const s = (span as { s?: number }).s;
        if (s == null) return null;
        return attributionLookup.get(s) ?? null;
    };

    // Auto-grow the inline input with its content (also shrinks back
    // after submit clears it).
    React.useEffect(() => {
        const ta = inlineInputRef.current;
        if (!ta) return;
        ta.style.height = 'auto';
        ta.style.height = `${ta.scrollHeight}px`;
    }, [commentText, showInlineInput]);

    // Close the inline input only once the submitted comment actually
    // lands in the list (the commit round-trip re-renders this block
    // with the new comment), not the moment Enter is pressed.
    const [closeAtCount, setCloseAtCount] = React.useState<number | null>(null);
    React.useEffect(() => {
        if (closeAtCount !== null && comments.length > closeAtCount) {
            setShowInlineInput(false);
            setCloseAtCount(null);
        }
    }, [comments.length, closeAtCount]);

    // A bubble self-expanded to show its comments has nothing left to show
    // once the last one is resolved (locally or by a collaborator):
    // collapse it so the chrome falls back to the hover-only `+`
    // affordance instead of rendering the expanded branch with zero rows
    // (an empty pill — bd-bpt089zw). `showInlineInput` guards the one
    // legitimate empty-and-expanded state: `+` just clicked, input open,
    // no comment yet. A LAYOUT effect so the correction re-renders before
    // paint — the zero-row pill is never on screen.
    React.useLayoutEffect(() => {
        if (comments.length === 0 && selfExpanded && !showInlineInput) {
            setSelfExpanded(false);
        }
    }, [comments.length, selfExpanded, showInlineInput]);

    // Focus the inline input when it opens, cursor at the end. (The
    // block editors see `data-q2-owns-focus` on their blur
    // relatedTarget and skip their focus-restore, so nothing steals
    // focus back.)
    React.useEffect(() => {
        const ta = inlineInputRef.current;
        if (showInlineInput && ta) {
            ta.focus();
            const end = ta.value.length;
            ta.setSelectionRange(end, end);
        }
    }, [showInlineInput]);

    // Clicking outside the bubble collapses a self-expanded bubble and
    // closes the inline input.
    React.useEffect(() => {
        if (!showInlineInput && !selfExpanded) return;
        const handleClickOutside = (event: MouseEvent) => {
            if (bubbleRef.current && !bubbleRef.current.contains(event.target as Node)) {
                setShowInlineInput(false);
                setSelfExpanded(false);
            }
        };
        document.addEventListener('mousedown', handleClickOutside);
        return () => { document.removeEventListener('mousedown', handleClickOutside); };
    }, [showInlineInput, selfExpanded]);

    // Only one self-expanded bubble at a time (module-level latch).
    React.useEffect(() => {
        if (!selfExpanded) return;
        collapseExpandedBubble?.();
        const collapse = () => {
            setSelfExpanded(false);
            setShowInlineInput(false);
        };
        collapseExpandedBubble = collapse;
        return () => {
            if (collapseExpandedBubble === collapse) collapseExpandedBubble = null;
        };
    }, [selfExpanded]);

    /**
     * Resolve the block to a committable source node. Null when
     * commenting here would corrupt the source: table cells resolve as
     * Opaque (the edit system can't commit there — same reason they
     * aren't click-editable), and transform products (figure captions →
     * Figure, def-list items → DefinitionList) round-trip lossily.
     */
    const resolveCommittable = () => {
        const resolved = edit.resolveSource(block);
        if (!resolved || !resolved.sourceNode) return null;
        if (resolved.reachabilityClass === 'Opaque') return null;
        if (!sameCommentableKind(block, resolved.sourceNode)) return null;
        return resolved;
    };

    // Remove the index-th comment span (counting comment spans only,
    // in order) from the source node and commit.
    const resolveCommentAtIndex = (index: number): void => {
        const resolved = resolveCommittable();
        if (!resolved) return;
        const modified = structuredClone(resolved.sourceNode);
        const removeNth = (arr: InlineNode[]) => {
            let seen = -1;
            for (let i = 0; i < arr.length; i++) {
                if (isComment(arr[i])) {
                    seen++;
                    if (seen === index) {
                        arr.splice(i, 1);
                        return;
                    }
                }
            }
        };
        const slot = inlineSlot(modified);
        if (slot) {
            removeNth(slot);
        } else if (isCommentContainer(modified)) {
            // Comments live across the container's paragraphs; count
            // them in order, remove the index-th, and drop a paragraph
            // that was left empty by the removal.
            const children = (modified as DivBlock).c[1];
            let seen = -1;
            outer:
            for (let ci = 0; ci < children.length; ci++) {
                const arr = inlineSlot(children[ci]);
                if (!arr) continue;
                for (let i = 0; i < arr.length; i++) {
                    if (isComment(arr[i])) {
                        seen++;
                        if (seen === index) {
                            arr.splice(i, 1);
                            if (arr.length === 0) children.splice(ci, 1);
                            break outer;
                        }
                    }
                }
            }
            // Last comment resolved with a single wrapped block left →
            // unwrap: commit the bare block in place of the container.
            const anyLeft = children.some((ch) =>
                (inlineSlot(ch) ?? []).some(isComment),
            );
            if (!anyLeft && children.length === 1) {
                edit.commitSubtreeEdit(JSON.stringify(resolved.sourceEntry), children[0]);
                return;
            }
        }
        edit.commitSubtreeEdit(JSON.stringify(resolved.sourceEntry), modified);
    };

    // Append a comment span to the source node and commit.
    const addComment = () => {
        const resolved = resolveCommittable();
        if (!resolved) return;
        const modified = structuredClone(resolved.sourceNode);
        const newComment: SpanInline = {
            t: 'Span',
            c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: commentText }]],
        };
        const slot = inlineSlot(modified);
        if (slot) {
            slot.push(newComment);
        } else if (modified.t === 'CodeBlock') {
            // A code block can't hold an inline span: wrap it in a
            // comment container Div with the comment as a `[>> ...]`
            // paragraph inside. Further comments append to that Div.
            const wrapper: DivBlock = {
                t: 'Div',
                c: [
                    ['', [CONTAINER_CLASS], []],
                    [modified, { t: 'Para', c: [newComment] } as ParaBlock],
                ],
            };
            edit.commitSubtreeEdit(JSON.stringify(resolved.sourceEntry), wrapper as BlockNode);
            setCommentText('');
            return;
        } else if (isCommentContainer(modified)) {
            // Append to the container's last comment paragraph, or add
            // a fresh one at the end.
            const children = (modified as DivBlock).c[1];
            const lastCommentPara = [...children].reverse().find((ch) =>
                (inlineSlot(ch) ?? []).some(isComment),
            );
            if (lastCommentPara) {
                inlineSlot(lastCommentPara)!.push(newComment);
            } else {
                children.push({ t: 'Para', c: [newComment] } as ParaBlock);
            }
        }
        edit.commitSubtreeEdit(JSON.stringify(resolved.sourceEntry), modified);
        setCommentText('');
    };

    const chromeVisible = comments.length > 0 || isHovered || selfExpanded;

    // Register this bubble with the force layout while visible. Mounts
    // (and size changes via the deps) reflow; unmounts deliberately
    // don't — a disappearing bubble leaves the arrangement as-is.
    React.useLayoutEffect(() => {
        if (!chromeVisible) return;
        const entry: BubbleEntry = {
            el: chromeRef.current,
            getAnchor: () => getAnchorRef.current(),
            glow: glowRef.current,
            nudge: nudgeRef.current,
            hovered: isHoveredRef.current,
            setNudge: (y) => {
                nudgeRef.current = y;
                setNudge(y);
            },
            observed: null,
            styledFrom: null,
            setInDeck,
        };
        entryRef.current = entry;
        bubbleEntries.add(entry);
        // Place before paint (no first-frame flash at the layer origin),
        // then let the batched pass solve overlaps.
        placeEntryNow(entry);
        scheduleBubbleRelayout();
        return () => {
            entryRef.current = null;
            bubbleEntries.delete(entry);
            observeAnchor(entry, null);
        };
        // expanded/showInlineInput change the bubble's size — re-register
        // so the force layout re-measures.
    }, [chromeVisible, comments.length, expanded, showInlineInput]);

    // Delegated hover (see `onDocumentMouseMove`): this block's record
    // lives for the whole life of the chrome-eligible block, so the `+`
    // affordance can appear on hover before any chrome exists. The
    // chrome/bubble elements are synced into it as they mount.
    const hoverRecordRef = React.useRef<HoverRecord | null>(null);
    React.useLayoutEffect(() => {
        const record: HoverRecord = {
            getAnchor: () => getAnchorRef.current(),
            anchorEl: null,
            chromeEl: null,
            bubbleEl: null,
            onPointer: ({ x, y, hovered, inBubble }) => {
                setIsHovered(hovered);
                lastPointerRef.current = { x, y };
                updateBubbleHovered(inBubble);
            },
            onLeave: () => {
                setIsHovered(false);
                lastPointerRef.current = null;
                updateBubbleHovered(false);
            },
        };
        hoverRecordRef.current = record;
        addHoverRecord(record);
        return () => {
            hoverRecordRef.current = null;
            removeHoverRecord(record);
        };
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);
    React.useLayoutEffect(() => {
        const record = hoverRecordRef.current;
        if (!record) return;
        // The anchor may have (re)attached with this render.
        indexAnchor(record);
        setRecordChrome(record, chromeVisible ? chromeRef.current : null, chromeVisible ? bubbleRef.current : null);
    });
    // The glow just mounted / unmounted: place it right away, then let
    // the pass keep it in step with the block.
    React.useLayoutEffect(() => {
        const entry = entryRef.current;
        if (!entry) return;
        entry.glow = glowRef.current;
        placeEntryNow(entry);
    }, [bubbleHovered, chromeVisible]);

    // The bubble just changed shape (same triggers as the re-register
    // above) under a possibly STATIONARY pointer — e.g. the ✓ row that was
    // under the cursor is gone, or the collapsed `+` now sits where the ✓
    // was. No pointer event will arrive to say so, so re-derive the block
    // glow from geometry, in both directions (bd-bpt089zw). The pointer
    // position is only known while it is over this block or its bubble
    // (recorded by the delegated mousemove, cleared on leave), so "inside the
    // re-measured bubble" is exactly "hovering the bubble".
    React.useLayoutEffect(() => {
        const pt = lastPointerRef.current;
        const r = chromeVisible ? bubbleRef.current?.getBoundingClientRect() : undefined;
        const inside =
            !!pt && !!r && pt.x >= r.left && pt.x <= r.right && pt.y >= r.top && pt.y <= r.bottom;
        updateBubbleHovered(inside);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [chromeVisible, comments.length, expanded, showInlineInput]);

    // Rich content can grow the bubble asynchronously — an <img> in a
    // comment finishes loading after the force layout measured the
    // chip. `load` doesn't bubble, so listen in the CAPTURE phase on
    // the chrome and re-solve when it fires (growth is bounded by the
    // .q2-comment-bubble img clamp + the chip's own maxWidth).
    React.useEffect(() => {
        if (!chromeVisible) return;
        const el = chromeRef.current;
        if (!el) return;
        const onDescendantLoad = () => scheduleBubbleRelayout();
        el.addEventListener('load', onDescendantLoad, true);
        return () => el.removeEventListener('load', onDescendantLoad, true);
    }, [chromeVisible]);

    // Sync hover into the registry entry. Re-layout on hover START
    // only: un-hovering keeps the arrangement as-is (it persists until
    // the next hover or a new bubble mounts).
    React.useEffect(() => {
        isHoveredRef.current = isHovered;
        if (entryRef.current && entryRef.current.hovered !== isHovered) {
            entryRef.current.hovered = isHovered;
            if (isHovered) scheduleBubbleRelayout();
        }
    }, [isHovered]);

    // Switching comments mode (e.g. back to un-expanded view) does a
    // full reset solve: bubbles return to their proper positions
    // instead of keeping accumulated push-only displacement.
    const prevModeRef = React.useRef(mode);
    React.useEffect(() => {
        if (prevModeRef.current !== mode) {
            prevModeRef.current = mode;
            scheduleBubbleRelayout(true);
        }
    }, [mode]);

    // The chrome (bubble + block glow) is portalled into the overlay
    // layer; the block itself renders untouched. No element ever sits
    // between the block and its parent (bd-q2wqj24c).
    const chrome = chromeVisible && (
        <>
            {bubbleHovered && (
                // Bubble hover glows the block, tying the two together:
                // an outline over the block's rect (placed by the layout
                // pass), never a style written onto the block.
                <div
                    ref={glowRef}
                    data-q2-comment-glow=""
                    style={{
                        position: 'absolute',
                        pointerEvents: 'none',
                        borderRadius: '3px',
                        boxShadow: GLOW,
                        zIndex: 99,
                    }}
                />
            )}
                <div
                    ref={chromeRef}
                    style={{
                        // `top`/`left` are written by the layout pass
                        // (placeEntry) from the block's rect; the -100%
                        // translation right-aligns the chrome on that
                        // point without measuring its width.
                        position: 'absolute',
                        pointerEvents: 'auto',
                        transform: `translate(-100%, ${nudge}px)${inDeck ? ` scale(${DECK_BUBBLE_SCALE})` : ''}`,
                        transformOrigin: 'top right',
                        // Animate nudge changes; the relayout pass reads
                        // the in-flight translation, so mid-animation
                        // reflows stay correct.
                        transition: 'transform 0.15s ease-out',
                        // Peer bubbles all sit at 100; a self-expanded
                        // bubble lifts further so no peer can paint
                        // above it.
                        zIndex: selfExpanded ? 1000 : 100,
                    }}
                    // Keep chrome interactions away from the delegated
                    // click-to-edit handler on the document root —
                    // otherwise clicking the bubble/input activates the
                    // enclosing block's editor. `data-q2-owns-focus`
                    // additionally marks this chrome as owning its
                    // focus: the block editors see it on blur
                    // relatedTarget and skip the focus-restore that
                    // would steal focus back from the comment input.
                    data-q2-owns-focus=""
                    onPointerDown={(e) => e.stopPropagation()}
                    onPointerUp={(e) => e.stopPropagation()}
                    onMouseDown={(e) => {
                        e.stopPropagation();
                        // Clicking non-interactive chrome (the bubble)
                        // must not blur an open block editor — the
                        // bubble's input takes focus itself once open.
                        if (!(e.target as HTMLElement).closest('input, textarea, button')) {
                            e.preventDefault();
                        }
                    }}
                    onClick={(e) => {
                        // Link clicks inside the bubble route through
                        // the preview's link policy here (they can't
                        // reach the delegated body listener past the
                        // stopPropagation below).
                        handleBubbleLinkClick(e);
                        e.stopPropagation();
                    }}
                    onKeyDown={(e) => e.stopPropagation()}
                >
                    <div
                        ref={bubbleRef}
                        className="q2-comment-bubble"
                        style={{
                            // Near-white with just a hint of blue.
                            backgroundColor: '#f7faff',
                            color: CHROME_BLUE,
                            borderRadius: '5px',
                            border: `1px solid ${CHROME_BLUE}`,
                            fontSize: expanded ? '0.75rem' : '0.7rem',
                            cursor: 'pointer',
                            // Chip-level containment (bd-y66gbfs4): no
                            // rendered comment content of any kind may
                            // widen the chip. Generous vs. the designed
                            // content widths (expanded rows ≈ 220px) so
                            // it only bites on oversized content.
                            maxWidth: '260px',
                            // The bubble stays shrink-to-fit: expanded
                            // rows are only as wide as their text. The
                            // add-comment input below sets its own
                            // min-width so the chrome can't collapse to
                            // a sliver when the textarea is all it holds.
                            boxSizing: 'border-box',
                            padding: expanded ? '4px 8px' : '2px 6px',
                            overflow: 'hidden',
                            // Block hover puts an offset-free light
                            // blue glow on the bubble.
                            boxShadow: isHovered ? GLOW : '0 2px 4px rgba(0,0,0,0.2)',
                            transition: 'box-shadow 0.15s',
                            userSelect: 'none',
                        }}
                        onClick={(e) => {
                            // A click on a link is a navigation, not a
                            // bubble interaction — the chrome-level
                            // handler routes it; skip expand/open.
                            if ((e.target as Element | null)?.closest?.('a')) return;
                            // Clicking a compact bubble expands it in
                            // place; any click opens the inline input.
                            if (!expanded) setSelfExpanded(true);
                            setShowInlineInput(true);
                        }}
                        title={`${comments.length} comment${comments.length !== 1 ? 's' : ''}`}
                    >
                        {comments.length === 0 && !expanded ? (
                            <div>+</div>
                        ) : expanded ? (
                            <>
                                {comments.map((c, i) => {
                                    const author = commentAuthor(c);
                                    return (
                                        <div key={i} style={{
                                            display: 'flex',
                                            alignItems: 'flex-start',
                                            gap: '6px',
                                            padding: '3px 2px',
                                            borderBottom: i < comments.length - 1 ? DIVIDER : 'none',
                                        }}>
                                            {author && (
                                                <span
                                                    title={author.name}
                                                    style={{
                                                        width: '8px',
                                                        height: '8px',
                                                        borderRadius: '50%',
                                                        backgroundColor: author.color,
                                                        display: 'inline-block',
                                                        flexShrink: 0,
                                                        // Align with the first text line.
                                                        marginTop: '3px',
                                                    }}
                                                />
                                            )}
                                            <span style={{
                                                flex: 1,
                                                minWidth: 0,
                                                maxWidth: '200px',
                                                overflowWrap: 'break-word',
                                                lineHeight: 1.4,
                                            }}>
                                                <CommentSpanContent span={c} onNavigateToDocument={onNavigateToDocument} />
                                            </span>
                                            <button
                                                onClick={(ev) => {
                                                    // Resolve without also
                                                    // triggering the bubble's
                                                    // own click handler.
                                                    ev.stopPropagation();
                                                    resolveCommentAtIndex(i);
                                                }}
                                                title="Resolve comment"
                                                style={{
                                                    padding: '0 4px',
                                                    backgroundColor: 'transparent',
                                                    color: CHROME_BLUE,
                                                    border: '1px solid #b3d9ff',
                                                    borderRadius: '4px',
                                                    fontSize: '0.65rem',
                                                    cursor: 'pointer',
                                                    flexShrink: 0,
                                                    transition: 'background-color 0.15s',
                                                }}
                                                onMouseEnter={(ev) => ev.currentTarget.style.backgroundColor = '#d4e8ff'}
                                                onMouseLeave={(ev) => ev.currentTarget.style.backgroundColor = 'transparent'}
                                            >
                                                ✓
                                            </button>
                                        </div>
                                    );
                                })}
                                {showInlineInput && (
                                    <div style={{
                                        borderTop: comments.length > 0 ? DIVIDER : 'none',
                                        marginTop: comments.length > 0 ? '4px' : 0,
                                        paddingTop: comments.length > 0 ? '6px' : '2px',
                                        paddingBottom: '2px',
                                    }}>
                                        <textarea
                                            ref={inlineInputRef}
                                            className="q2-comment-input"
                                            rows={2}
                                            value={commentText}
                                            onChange={(e) => setCommentText(e.target.value)}
                                            onKeyDown={(e) => {
                                                if (e.key === 'Enter') {
                                                    e.preventDefault();
                                                    if (commentText) {
                                                        addComment();
                                                        // Close once the comment
                                                        // shows up in the list.
                                                        setCloseAtCount(comments.length);
                                                    }
                                                } else if (e.key === 'Escape') {
                                                    setShowInlineInput(false);
                                                    setSelfExpanded(false);
                                                }
                                            }}
                                            placeholder="Add a comment…"
                                            style={{
                                                display: 'block',
                                                width: '100%',
                                                // The chrome is a shrink-to-fit
                                                // box; with no comment rows the
                                                // textarea is the only thing
                                                // giving it a width.
                                                minWidth: '200px',
                                                padding: '5px 7px',
                                                fontFamily: 'inherit',
                                                fontSize: 'inherit',
                                                lineHeight: 1.4,
                                                // Blend into the bubble like
                                                // the comment rows do.
                                                backgroundColor: 'transparent',
                                                color: 'inherit',
                                                border: DIVIDER,
                                                borderRadius: '4px',
                                                outline: 'none',
                                                resize: 'none',
                                                overflow: 'hidden',
                                                boxSizing: 'border-box',
                                            }}
                                        />
                                        <div style={{
                                            marginTop: '3px',
                                            fontSize: '0.85em',
                                            color: '#6699cc',
                                            textAlign: 'right',
                                        }}>
                                            Enter to add · Esc to close
                                        </div>
                                    </div>
                                )}
                            </>
                        ) : (
                            <>
                                <div style={{
                                    maxWidth: '140px',
                                    overflow: 'hidden',
                                    whiteSpace: 'nowrap',
                                    textOverflow: 'ellipsis',
                                }}>
                                    <CommentSpanContent span={comments[0]} onNavigateToDocument={onNavigateToDocument} />
                                </div>
                                {comments.length > 1 && (
                                    <div style={{ color: '#6699cc', textAlign: 'right' }}>
                                        +{comments.length - 1} more
                                    </div>
                                )}
                            </>
                        )}
                    </div>
                </div>
        </>
    );

    return (
        <CommentAnchorContext.Provider value={anchorTarget}>
            {chrome ? createPortal(chrome, getCommentLayer()) : null}
            {children}
        </CommentAnchorContext.Provider>
    );
};

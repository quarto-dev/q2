/**
 * Default comment chrome for q2-preview blocks.
 *
 * Comments are `[>> ...]` editorial-mark spans (class
 * `quarto-edit-comment`) stored inline in the block's own source. This
 * component extracts them before rendering and shows them as a small
 * "bubble" anchored to the block's bottom-right corner:
 *
 *  - Compact bubble ('show' mode): first comment preview + "+n more";
 *    a comment-less block shows a "+" add affordance on hover.
 *  - Expanded bubble (global 'expand' mode, or any bubble clicked
 *    open): every comment as its own row with author dot (when the
 *    Authors overlay provides attribution), a ✓ resolve button, and an
 *    inline add-comment input at the bottom.
 *  - 'hide' mode strips comments from the text but renders no chrome.
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
import { AttributionLookupContext } from '../../framework';
import type {
    BlockNode,
    DivBlock,
    HeaderBlock,
    InlineNode,
    NodeArgs,
    ParaBlock,
    PlainBlock,
    SpanInline,
    StrInline,
} from '../../framework';
import { Block as B } from '../dispatchers';
import { PreviewContext } from '../PreviewContext';
import type { CommentsMode } from '../PreviewContext';
import { usePreviewEdit } from '../usePreviewEdit';

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
        '.q2-comment-input::placeholder { color: #a9c7e8; opacity: 1; }';
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

function commentSpanText(span: InlineNode): string {
    return (span as SpanInline).c[1]
        .map((o: InlineNode) => {
            if (o.t === 'Str') return (o as StrInline).c;
            if (o.t === 'Space') return ' ';
            return '';
        })
        .join('');
}

export const CommentBlock = (args: NodeArgs<BlockNode>) => {
    const edit = usePreviewEdit();
    const insideContainer = React.useContext(InsideCommentContainer);
    const mode: CommentsMode =
        React.useContext(PreviewContext)?.commentsMode ?? 'show';
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

    // Commenting on figures, definition lists, and table cells is
    // BUSTED right now: committing a Figure or DefinitionList source
    // node re-serializes it lossily (broken syntax), and table cells
    // are Opaque (writes never land). Hide the bubble there entirely —
    // addComment/resolveCommentAtIndex refuse the same cases as a
    // backstop. Blocks that already carry comments keep their bubble
    // for read-only display.
    if (comments.length === 0) {
        const resolved = edit.resolveSource(block);
        if (
            !resolved ||
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
    // (and no wrapper div) renders at all.
    if (mode === 'hide') return content;
    return (
        <CommentWrapper comments={comments} block={block} edit={edit} mode={mode}>
            {content}
        </CommentWrapper>
    );
};

type EditHandle = ReturnType<typeof usePreviewEdit>;

// Only one self-expanded bubble at a time: expanding one collapses the
// previously expanded one via this module-level latch.
let collapseExpandedBubble: (() => void) | null = null;

// ---------------------------------------------------------------------
// Tiny force layout. Visible bubbles register here; a batched rAF pass
// keeps them from overlapping, under these rules:
//  - the hovered bubble is pinned at its natural spot (nudged below the
//    viewport top if needed) and everything else moves around it;
//  - pushes are directional in DOCUMENT order (earlier bubbles only get
//    pushed up, later ones only down), so bubbles never reorder;
//  - idle bubbles keep their displacement between passes (push-only),
//    but drift back toward natural whenever free space allows;
//  - a comments-mode switch does a full reset solve from naturals.
const BUBBLE_GAP = 4;
type BubbleEntry = {
    el: HTMLElement | null;
    nudge: number;
    /** Block currently hovered — its bubble is pinned at its natural spot. */
    hovered: boolean;
    setNudge: (y: number) => void;
};
const bubbleEntries = new Set<BubbleEntry>();
let bubbleRelayoutScheduled = false;
// When set, the next pass solves from NATURAL positions (full reset)
// instead of from current nudges (push-only). OR-ed across schedule
// calls in the same frame.
let bubbleRelayoutReset = false;

/** The translateY currently painted on the element. During a transform
 *  transition this is the in-flight value, not the target nudge — the
 *  relayout pass subtracts it to recover the natural position. */
function appliedTranslateY(el: HTMLElement, fallback: number): number {
    try {
        const t = getComputedStyle(el).transform;
        if (!t || t === 'none') return 0;
        const m = t.match(/matrix\(([^)]+)\)/);
        if (!m) return fallback;
        return parseFloat(m[1].split(',')[5]) || 0;
    } catch {
        return fallback;
    }
}

function scheduleBubbleRelayout(reset = false) {
    if (reset) bubbleRelayoutReset = true;
    if (bubbleRelayoutScheduled) return;
    bubbleRelayoutScheduled = true;
    requestAnimationFrame(() => {
        bubbleRelayoutScheduled = false;
        const resetPass = bubbleRelayoutReset;
        bubbleRelayoutReset = false;
        const items = [...bubbleEntries]
            .filter((e) => e.el)
            .map((e) => {
                const rect = e.el!.getBoundingClientRect();
                // Subtract the currently painted translation to get the
                // bubble's natural (untranslated) position.
                const top = rect.top - appliedTranslateY(e.el!, e.nudge);
                // HOVER PIN: the hovered bubble sits at its natural
                // position (same spot every time, overlapping its
                // block, ready to click) — except it may never sit
                // above the top of the PAGE (a tall expanded bubble on
                // the first block would otherwise be unreachable); its
                // pin shifts down just enough. Going above the viewport
                // top when scrolled is fine. Idle bubbles float freely.
                const TOP_MARGIN = 8;
                // Page-top expressed in viewport coords (rects are
                // viewport-relative).
                const pageTop = TOP_MARGIN - window.scrollY;
                const pinnedTop = Math.max(top, pageTop);
                const clamp = e.hovered ? () => pinnedTop : (y: number) => y;
                // Idle bubbles START from their current (already-nudged)
                // position — a relayout only ever PUSHES them further,
                // never pulls them back (the settle phase below handles
                // drifting home). A reset pass starts from naturals.
                return {
                    e,
                    top,
                    height: rect.height,
                    left: rect.left,
                    right: rect.right,
                    cur: e.hovered ? pinnedTop : resetPass ? top : top + e.nudge,
                    clamp,
                    pinned: e.hovered,
                };
            })
            // DOCUMENT order, not visual order: pushes are directional
            // relative to it (earlier-in-document bubbles may only be
            // pushed UP, later ones only DOWN), so document order can
            // never be visually inverted by the layout.
            .sort((a, b) =>
                a.e.el!.compareDocumentPosition(b.e.el!) & Node.DOCUMENT_POSITION_FOLLOWING
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
    edit,
    mode,
}: {
    children: React.ReactNode;
    comments: InlineNode[];
    block: BlockNode;
    edit: EditHandle;
    mode: CommentsMode;
}) => {
    const [commentText, setCommentText] = React.useState('');
    // Clicking a compact bubble expands it in place (with the inline
    // add-comment input open at its bottom).
    const [selfExpanded, setSelfExpanded] = React.useState(false);
    const [showInlineInput, setShowInlineInput] = React.useState(false);
    const inlineInputRef = React.useRef<HTMLTextAreaElement>(null);
    const [isHovered, setIsHovered] = React.useState(false);
    // Hovering the bubble itself glows the block (mirror of the
    // block-hover → bubble-glow effect).
    const [bubbleHovered, setBubbleHovered] = React.useState(false);
    // Global 'expand' mode expands every commented bubble; a click
    // self-expands one bubble in any mode.
    const expanded = (mode === 'expand' && comments.length > 0) || selfExpanded;
    // Force-layout nudge (translateY) keeping this bubble clear of its
    // neighbors; nudgeRef mirrors it for the module-level relayout pass.
    const [nudge, setNudge] = React.useState(0);
    const nudgeRef = React.useRef(0);
    const chromeRef = React.useRef<HTMLDivElement>(null);
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
        if (!resolved) return null;
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
            nudge: nudgeRef.current,
            hovered: isHoveredRef.current,
            setNudge: (y) => {
                nudgeRef.current = y;
                setNudge(y);
            },
        };
        entryRef.current = entry;
        bubbleEntries.add(entry);
        scheduleBubbleRelayout();
        return () => {
            entryRef.current = null;
            bubbleEntries.delete(entry);
        };
        // expanded/showInlineInput change the bubble's size — re-register
        // so the force layout re-measures.
    }, [chromeVisible, comments.length, expanded, showInlineInput]);

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

    return (
        <div
            style={{
                position: 'relative',
                // Bubble hover glows the block, tying the two together.
                boxShadow: bubbleHovered ? GLOW : 'none',
                transition: 'box-shadow 0.15s',
            }}
            // Only the RIGHT half of the block counts as hover (the
            // bubble lives at the right edge) — mousing across the left
            // half while reading doesn't reveal chrome or reshuffle the
            // bubble layout.
            onMouseMove={(e) => {
                const rect = e.currentTarget.getBoundingClientRect();
                setIsHovered(e.clientX >= rect.left + rect.width / 2);
            }}
            onMouseLeave={() => setIsHovered(false)}
        >
            {/* Chrome renders BEFORE the content: mounting it as a
                LAST sibling on hover would stop the content matching
                theme `:last-child` rules (e.g. the last paragraph in a
                blockquote loses margin-bottom: 0 and the quote grows —
                a hover reflow). It's absolutely positioned, so DOM
                order doesn't change where it paints. */}
            {chromeVisible && (
                <div
                    ref={chromeRef}
                    style={{
                        position: 'absolute',
                        top: '-11px',
                        right: '-10px',
                        transform: `translateY(${nudge}px)`,
                        // Animate nudge changes; the relayout pass reads
                        // the in-flight translation, so mid-animation
                        // reflows stay correct.
                        transition: 'transform 0.15s ease-out',
                        // The bubble pokes above the block's box, into
                        // the previous (positioned) sibling wrapper —
                        // lift it above so it wins hit-testing there. A
                        // self-expanded bubble lifts further so no peer
                        // bubble (all at 100) can paint above it.
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
                    onClick={(e) => e.stopPropagation()}
                    onKeyDown={(e) => e.stopPropagation()}
                >
                    <div
                        ref={bubbleRef}
                        style={{
                            // Near-white with just a hint of blue.
                            backgroundColor: '#f7faff',
                            color: CHROME_BLUE,
                            padding: '2px 6px',
                            borderRadius: '5px',
                            border: `1px solid ${CHROME_BLUE}`,
                            fontSize: '0.7rem',
                            cursor: 'pointer',
                            // Block hover puts an offset-free light
                            // blue glow on the bubble.
                            boxShadow: isHovered ? GLOW : '0 2px 4px rgba(0,0,0,0.2)',
                            transition: 'box-shadow 0.15s',
                            userSelect: 'none',
                        }}
                        onClick={() => {
                            // Clicking a compact bubble expands it in
                            // place; any click opens the inline input.
                            if (!expanded) setSelfExpanded(true);
                            setShowInlineInput(true);
                        }}
                        onMouseEnter={() => setBubbleHovered(true)}
                        onMouseLeave={() => setBubbleHovered(false)}
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
                                                maxWidth: '160px',
                                                overflowWrap: 'break-word',
                                            }}>
                                                {commentSpanText(c)}
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
                                        marginTop: '2px',
                                        paddingTop: '5px',
                                        paddingBottom: '2px',
                                    }}>
                                        <textarea
                                            ref={inlineInputRef}
                                            className="q2-comment-input"
                                            rows={1}
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
                                            placeholder="type comment here"
                                            style={{
                                                display: 'block',
                                                width: '100%',
                                                padding: '3px 2px',
                                                fontFamily: 'inherit',
                                                fontSize: 'inherit',
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
                                    {commentSpanText(comments[0])}
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
            )}

            {children}
        </div>
    );
};

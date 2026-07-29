/**
 * Default comment chrome for q2-preview blocks. Promoted from the
 * render-components-comment playwright fixture
 * (`crates/quarto/tests/playwright-fixtures/q2-preview/render-components-comment/comment.tsx`)
 * so every preview gets it without a user render-components file.
 *
 * Extracts `quarto-edit-comment` spans out of a Para/Header's inlines
 * before rendering and shows them as corner chrome: a comment-count
 * button opening a list with per-comment "Resolve" (delete) and an
 * add-comment input. The chrome only appears while hovering the block,
 * unless the block already has comments. Adds/resolves round-trip
 * through `usePreviewEdit().commitSubtreeEdit`, which is a no-op where
 * `PreviewContext` is absent.
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

function isComment(inline: InlineNode): boolean {
    if (inline.t === 'Span' && 'c' in inline) {
        const attrs = (inline as SpanInline).c[0];
        const classes = attrs[1];
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

// Light blue placeholder tint for the comment inputs — ::placeholder
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
 * would rewrite lossily (bd: figures became `::: {#fig-..}` divs,
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

    // Children of a comment container defer to the container's bubble.
    if (insideContainer) {
        return <B node={block} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst} />;
    }

    let comments: InlineNode[] = [];
    let newBlock = block;
    // Casts: the BlockNode union includes `UnknownBlock { t: string }`,
    // which defeats discriminant narrowing on `block.t`.
    if (block.t === 'Para' || block.t === 'Plain') {
        const b = block as ParaBlock | PlainBlock;
        comments = b.c.filter(isComment);
        const clone = structuredClone(b);
        clone.c = b.c.filter((n) => !isComment(n));
        newBlock = clone;
    } else if (block.t === 'Header') {
        const b = block as HeaderBlock;
        comments = b.c[2].filter(isComment);
        const clone = structuredClone(b);
        clone.c[2] = b.c[2].filter((n) => !isComment(n));
        newBlock = clone;
    } else if (isCommentContainer(block)) {
        // Collect comment spans from the container's paragraphs and
        // strip them (dropping paragraphs that were nothing but
        // comments) so only the wrapped content renders.
        const clone = structuredClone(block) as DivBlock;
        const kept: BlockNode[] = [];
        for (const child of clone.c[1]) {
            if (child.t === 'Para' || child.t === 'Plain') {
                const p = child as ParaBlock | PlainBlock;
                comments.push(...p.c.filter(isComment));
                p.c = p.c.filter((n) => !isComment(n));
                if (p.c.length === 0) continue;
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
        block.t === 'Para' || block.t === 'Plain' || block.t === 'Header' ||
        block.t === 'CodeBlock' || isCommentContainer(block);
    if (!canHoldComment) {
        return <B node={block} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst} />;
    }

    // Commenting on figures, definition lists, and table cells is
    // BUSTED right now. Writing Figures cause broken syntax, 
    // writing definition lists cause broke syntax, table cells just dont write.
    if (comments.length === 0) {
        const resolved = edit.resolveSource(block);
        if (
            !resolved ||
            resolved.reachabilityClass === 'Opaque' ||
            !sameCommentableKind(block, resolved.sourceNode)
        ) {
            return <B node={block} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst} />;
        }
    }

    const inner = (
        <B node={newBlock} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst} />
    );
    const maybeProvided = isCommentContainer(block) ? (
        <InsideCommentContainer.Provider value={true}>
            {inner}
        </InsideCommentContainer.Provider>
    ) : (
        inner
    );
    // 'hide' mode: comments stay stripped from the text, but no chrome
    // (and no wrapper div) renders at all.
    if (mode === 'hide') {
        return maybeProvided;
    }
    return (
        <CommentWrapper comments={comments} block={block} edit={edit} mode={mode}>
            {maybeProvided}
        </CommentWrapper>
    );
};

type EditHandle = ReturnType<typeof usePreviewEdit>;

// Only one comments popup may be open at a time. Whichever popup opens
// closes the previously open one via this module-level latch.
let closeOpenPopup: (() => void) | null = null;

// ---------------------------------------------------------------------
// Tiny force layout: visible bubbles register here; after any of them
// mounts/unmounts, a rAF pass sorts them by natural position and nudges
// later ones down (translateY) just enough to clear earlier ones.
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

function scheduleBubbleRelayout() {
    if (bubbleRelayoutScheduled) return;
    bubbleRelayoutScheduled = true;
    requestAnimationFrame(() => {
        bubbleRelayoutScheduled = false;
        const items = [...bubbleEntries]
            .filter((e) => e.el)
            .map((e) => {
                const rect = e.el!.getBoundingClientRect();
                // Subtract the currently painted translation to get the
                // bubble's natural (untranslated) position.
                const top = rect.top - appliedTranslateY(e.el!, e.nudge);
                const height = rect.height;
                // HOVER PIN: while its block is hovered, the bubble is
                // pinned at its exact natural position (always the same
                // spot, overlapping its block, ready to click); the
                // relaxation pushes every other bubble around it. Idle
                // bubbles float freely to resolve overlaps, EXCEPT they
                // may never be pushed above the viewport top (a bubble
                // whose natural spot is already above it stays put —
                // the floor only limits upward nudging).
                const TOP_MARGIN = 8;
                const floor = Math.min(top, TOP_MARGIN);
                const clamp = e.hovered
                    ? () => top
                    : (y: number) => Math.max(y, floor);
                return { e, top, height, left: rect.left, right: rect.right, cur: top, clamp };
            })
            .sort((a, b) => a.top - b.top);
        // Iterative relaxation: each overlapping pair splits the push —
        // the earlier bubble moves up, the later one down — clamped to
        // each bubble's own-block range. When one side hits its clamp,
        // later rounds shift the remaining overlap onto the other side,
        // so the earlier bubble keeps moving up as far as it's allowed.
        for (let iter = 0; iter < 20; iter++) {
            let moved = false;
            for (let i = 0; i < items.length; i++) {
                for (let j = i + 1; j < items.length; j++) {
                    const a = items[i];
                    const b = items[j];
                    const overlapsH = a.left < b.right && b.left < a.right;
                    if (!overlapsH) continue;
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
    // No modal: clicking a compact bubble expands it in place (with the
    // inline add-comment input open at its bottom).
    const [selfExpanded, setSelfExpanded] = React.useState(false);
    const [showInlineInput, setShowInlineInput] = React.useState(false);
    const inlineInputRef = React.useRef<HTMLTextAreaElement>(null);
    const [isHovered, setIsHovered] = React.useState(false);
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

    const commentsListRef = React.useRef<HTMLDivElement>(null);

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
            if (commentsListRef.current && !commentsListRef.current.contains(event.target as Node)) {
                setShowInlineInput(false);
                setSelfExpanded(false);
            }
        };
        document.addEventListener('mousedown', handleClickOutside);
        return () => { document.removeEventListener('mousedown', handleClickOutside); };
    }, [showInlineInput, selfExpanded]);

    // Only one self-expanded bubble at a time: expanding one collapses
    // the previously expanded one via the module-level latch.
    React.useEffect(() => {
        if (!selfExpanded) return;
        closeOpenPopup?.();
        const close = () => {
            setSelfExpanded(false);
            setShowInlineInput(false);
        };
        closeOpenPopup = close;
        return () => {
            if (closeOpenPopup === close) closeOpenPopup = null;
        };
    }, [selfExpanded]);

    // Remove the index-th comment span (counting comment spans only,
    // in order) from the source node and commit.
    const resolveCommentAtIndex = (index: number): void => {
        const resolved = edit.resolveSource(block);
        if (!resolved) return;
        // Same safety gates as addComment: no structurally unreachable
        // targets (table cells), no transform products the writer
        // would re-serialize lossily (figures, definition lists).
        if (resolved.reachabilityClass === 'Opaque') return;
        if (!sameCommentableKind(block, resolved.sourceNode)) return;
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
        if (modified.t === 'Para' || modified.t === 'Plain') {
            removeNth((modified as ParaBlock | PlainBlock).c);
        } else if (modified.t === 'Header') {
            removeNth((modified as HeaderBlock).c[2]);
        } else if (isCommentContainer(modified)) {
            // Comments live across the container's paragraphs; count
            // them in order, remove the index-th, and drop a paragraph
            // that was left empty by the removal.
            const children = (modified as DivBlock).c[1];
            let seen = -1;
            outer:
            for (let ci = 0; ci < children.length; ci++) {
                const ch = children[ci];
                if (ch.t !== 'Para' && ch.t !== 'Plain') continue;
                const arr = (ch as ParaBlock | PlainBlock).c;
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
            const anyLeft = children.some(
                (ch) =>
                    (ch.t === 'Para' || ch.t === 'Plain') &&
                    (ch as ParaBlock | PlainBlock).c.some(isComment),
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
        const resolved = edit.resolveSource(block);
        if (!resolved) return;
        // Safety gates: table cells resolve as Opaque (the edit system
        // can't commit there — same reason they aren't click-editable),
        // and transform products (figure captions → Figure, def-list
        // items → DefinitionList) would round-trip lossily. Bail
        // silently rather than corrupt the source.
        if (resolved.reachabilityClass === 'Opaque') return;
        if (!sameCommentableKind(block, resolved.sourceNode)) return;
        const modified = structuredClone(resolved.sourceNode);
        const newComment: SpanInline = {
            t: 'Span',
            c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: commentText }]],
        };
        if (modified.t === 'Para' || modified.t === 'Plain') {
            (modified as ParaBlock | PlainBlock).c.push(newComment);
        } else if (modified.t === 'Header') {
            (modified as HeaderBlock).c[2].push(newComment);
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
            const lastCommentPara = [...children].reverse().find(
                (ch) =>
                    (ch.t === 'Para' || ch.t === 'Plain') &&
                    (ch as ParaBlock | PlainBlock).c.some(isComment),
            );
            if (lastCommentPara) {
                (lastCommentPara as ParaBlock | PlainBlock).c.push(newComment);
            } else {
                children.push({ t: 'Para', c: [newComment] } as ParaBlock);
            }
        }
        edit.commitSubtreeEdit(JSON.stringify(resolved.sourceEntry), modified);
        setCommentText('');
    };

    const hasContent = comments.length > 0;
    const chromeVisible = hasContent || isHovered || selfExpanded;

    // Register this bubble with the force layout while visible; any
    // mount/unmount (including hover chrome) reflows all bubbles.
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
            scheduleBubbleRelayout();
        };
        // expanded/showInlineInput change the bubble's size — re-register
        // so the force layout re-measures.
    }, [chromeVisible, comments.length, expanded, showInlineInput]);

    // Sync hover into the registry entry — the own-block clamp only
    // applies while the block is hovered, and hover changes reflow.
    React.useEffect(() => {
        isHoveredRef.current = isHovered;
        if (entryRef.current && entryRef.current.hovered !== isHovered) {
            entryRef.current.hovered = isHovered;
            scheduleBubbleRelayout();
        }
    }, [isHovered]);

    return (
        <div
            style={{ position: 'relative' }}
            onMouseEnter={() => setIsHovered(true)}
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
                        bottom: '-11px',
                        right: '-10px',
                        transform: `translateY(${nudge}px)`,
                        // Animate nudge changes; the relayout pass reads
                        // the in-flight translation, so mid-animation
                        // reflows stay correct.
                        transition: 'transform 0.15s ease-out',
                        display: 'flex',
                        flexDirection: 'row',
                        gap: '4px',
                        alignItems: 'center',
                        // The bubble hangs below the block's box, into
                        // the next (positioned) sibling wrapper — lift
                        // it above so it wins hit-testing there. A
                        // self-expanded bubble lifts further so no peer
                        // bubble (all at 100) can paint above it.
                        zIndex: selfExpanded ? 1000 : 100,
                    }}
                    // Keep chrome interactions away from the delegated
                    // click-to-edit handler on the document root —
                    // otherwise clicking the bubble/input activates the
                    // enclosing block's editor.
                    // Marks this chrome as owning its focus: the block
                    // editors see it on blur relatedTarget and skip
                    // their focus-restore (which would steal focus back
                    // from the comment input).
                    data-q2-owns-focus=""
                    onPointerDown={(e) => e.stopPropagation()}
                    onPointerUp={(e) => e.stopPropagation()}
                    onMouseDown={(e) => {
                        e.stopPropagation();
                        // Clicking non-interactive chrome (the bubble)
                        // must not blur an open block editor — the
                        // modal takes focus itself once open.
                        if (!(e.target as HTMLElement).closest('input, textarea, button')) {
                            e.preventDefault();
                        }
                    }}
                    onClick={(e) => e.stopPropagation()}
                    onKeyDown={(e) => e.stopPropagation()}
                >
                    <div ref={commentsListRef} style={{ position: 'relative' }}>
                        <div
                            style={{
                                // Near-white with just a hint of blue.
                                backgroundColor: '#f7faff',
                                color: '#4a7ba7',
                                padding: '2px 6px',
                                borderRadius: '5px',
                                border: '1px solid #4a7ba7',
                                fontSize: '0.7rem',
                                cursor: 'pointer',
                                // Block hover puts an offset-free light
                                // blue glow on the bubble.
                                boxShadow: isHovered
                                    ? '0 0 8px 2px rgba(140, 190, 240, 0.6)'
                                    : '0 2px 4px rgba(0,0,0,0.2)',
                                transition: 'box-shadow 0.15s',
                                userSelect: 'none',
                            }}
                            onClick={() => {
                                // Clicking a compact bubble expands it
                                // in place; any click opens the inline
                                // add-comment input.
                                if (!expanded) setSelfExpanded(true);
                                setShowInlineInput(true);
                            }}
                            title={`${comments.length} comment${comments.length !== 1 ? 's' : ''}`}
                        >
                            {/* An expanded bubble (global 'expand' mode or
                                clicked-open) lists every comment as its own
                                row with dividers, plus the inline input; a
                                compact bubble previews the first comment
                                and sums the rest as "+n more". A
                                comment-less compact bubble is just the "+"
                                add affordance. */}
                            {comments.length === 0 && !expanded ? (
                                <div>+</div>
                            ) : expanded ? (
                                <>
                                {comments.map((c, i) => {
                                    const author = commentAuthor(c);
                                    return (
                                    <div key={i} style={{
                                        display: 'flex',
                                        alignItems: 'center',
                                        gap: '6px',
                                        padding: '3px 2px',
                                        borderBottom: i < comments.length - 1
                                            ? '1px solid rgba(74, 123, 167, 0.3)'
                                            : 'none',
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
                                                }}
                                            />
                                        )}
                                        <span style={{
                                            flex: 1,
                                            minWidth: 0,
                                            maxWidth: '160px',
                                            overflow: 'hidden',
                                            whiteSpace: 'nowrap',
                                            textOverflow: 'ellipsis',
                                        }}>
                                            {commentSpanText(c)}
                                        </span>
                                        <button
                                            onClick={(ev) => {
                                                // Resolve without toggling
                                                // the modal (bubble onClick).
                                                ev.stopPropagation();
                                                resolveCommentAtIndex(i);
                                            }}
                                            title="Resolve comment"
                                            style={{
                                                padding: '0 4px',
                                                backgroundColor: 'transparent',
                                                color: '#4a7ba7',
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
                                        borderTop: '1px solid rgba(74, 123, 167, 0.3)',
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
                                                border: '1px solid rgba(74, 123, 167, 0.3)',
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
                </div>
            )}

            {children}
        </div>
    );
};

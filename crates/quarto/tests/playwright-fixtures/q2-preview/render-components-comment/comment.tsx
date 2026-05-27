const React = window.React;
const {
    Block: B
} = window.__Q2_PREVIEW_RENDERER__;

// Phase 1 diagnostic for the reactji-authorship demo plan
// (claude-notes/plans/2026-05-25-reactji-authorship-q2-preview.md).
// Surfaces what user TSX can see of the attribution surface so the
// e2e spec can prove which plumbing gaps exist on `feature/provenance`
// today. Removed (or gated) once Phase 2 closes the gaps.
// Idempotent under multiple module loads. The Phase 1 fields are the
// stable surface the e2e spec inspects; the surface snapshot is
// refreshed each load so the latest renderer-exports are visible.
const _diagBootstrap = ((window as any).__COMMENT_DIAG__ ??= {
    me: null,
    blocks: [],
});
_diagBootstrap.surfaceKeys = Object.keys((window as any).__Q2_PREVIEW_RENDERER__ ?? {});
_diagBootstrap.hasUseNodeAttribution = typeof (window as any).__Q2_PREVIEW_RENDERER__?.useNodeAttribution === 'function';
_diagBootstrap.hasUseCurrentActor = typeof (window as any).__Q2_PREVIEW_RENDERER__?.useCurrentActor === 'function';

// Conditional sub-component: mounted only when both attribution hooks
// are present on the renderer surface. Hook calls inside React must be
// unconditional, so the *mount* of this component is the gate, not the
// call sites within it.
const Diagnostic = ({ first }: { first: InlineNode | null }) => {
    const renderer = (window as any).__Q2_PREVIEW_RENDERER__;
    const me = renderer.useCurrentActor();
    const firstAttr = renderer.useNodeAttribution(first ?? undefined);
    React.useEffect(() => {
        const diag = (window as any).__COMMENT_DIAG__;
        diag.me = me;
        diag.blocks ??= [];
        diag.blocks.push({
            firstCommentText: first && (first as SpanInline).c
                ? (first as SpanInline).c[1].map((o: InlineNode) => o.t === 'Str' ? (o as StrInline).c : '').join('')
                : null,
            firstCommentS: (first as any)?.s ?? null,
            firstCommentAttr: firstAttr,
        });
    });
    return null;
};

function isComment(inline: InlineNode): boolean {
    if (inline.t === 'Span' && 'c' in inline) {
        const attrs = (inline as SpanInline).c[0];
        const classes = attrs[1];
        return classes.includes('quarto-edit-comment');
    }
    return false;
}

// export const Block = B
// BlockWithComments component
const splitEmoji = (string: string) => [...new Intl.Segmenter().segment(string)].map(x => x.segment)
export const Block = (args: NodeArgs<BlockNode>) => {
    const { node: block, onNavigateToDocument, setLocalAst } = args
    // Gather comments from inline children if block has them
    let comments: InlineNode[] = [];
    let newBlock = block
    if ('c' in block && block.c) {
        // For Para, Plain: c is Inline[]
        if ((block.t === 'Para' || block.t === 'Plain') && Array.isArray(block.c)) {
            comments = block.c.filter(isComment);
            newBlock = structuredClone(block)
            newBlock.c = block.c.filter((n: any) => !isComment(n));
        }
        // For Header: c is [number, [string, string[], [string, string][]], Inline[]]
        else if (block.t === 'Header' && Array.isArray(block.c) && Array.isArray(block.c[2])) {
            comments = block.c[2].filter(isComment);
            newBlock = structuredClone(block)
            //@ts-ignore
            newBlock.c[2] = block.c[2].filter((n: any) => !isComment(n));
        }
    }

    const commentContents = comments.map((c) => (c as SpanInline).c[1].map((o: InlineNode) => {
        if (o.t === 'Str') return (o as StrInline).c;
        if (o.t === 'Space') return ' ';
        return '';
    }).join(''))
    // Separate single-emoji reactjis from text comments. `reactionSpans`
    // keeps the original Span inlines (with their `s` source-info pool
    // index) so the Diagnostic + future authorship-aware click can look
    // up attribution on the actual span.
    const reactionSpans: InlineNode[] = comments.filter((_, i) => splitEmoji(commentContents[i]).length === 1);
    const reactions = commentContents.filter(c => splitEmoji(c).length === 1)
    const reactionCounts = reactions.reduce((acc, emoji) =>
        acc.set(emoji, (acc.get(emoji) || 0) + 1),
        new Map<string, number>()
    );
    comments = comments.filter((_, i) => splitEmoji(commentContents[i]).length !== 1)

    // Skip CommentWrapper for BulletList and OrderedList
    if (block.t === 'BulletList' || block.t === 'OrderedList') {
        return <B node={block} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst}></B>;
    }

    return <CommentWrapper reactionCounts={reactionCounts} reactionSpans={reactionSpans} comments={comments} setLocalAst={setLocalAst} block={block}>
        <B node={newBlock} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst}></B>
    </CommentWrapper>;
};

/**
 * CommentWrapper renders children in a box and displays gathered comments
 */
const CommentWrapper = ({ children, comments, reactionSpans, reactionCounts, setLocalAst, block }: { children: React.ReactNode, reactionCounts: Map<String, number>, reactionSpans: InlineNode[], comments: InlineNode[], setLocalAst: (newBlock: BlockNode) => void, block: BlockNode }) => {
    const [commentText, setCommentText] = React.useState('');
    const [showEmojiPicker, setShowEmojiPicker] = React.useState(false);
    const [showCommentsList, setShowCommentsList] = React.useState(false);
    const [isHovered, setIsHovered] = React.useState(false);
    const emojiPickerRef = React.useRef<HTMLDivElement>(null);
    const commentsListRef = React.useRef<HTMLDivElement>(null);
    const commentInputRef = React.useRef<HTMLInputElement>(null);

    // Close emoji picker when clicking outside
    React.useEffect(() => {
        if (!showEmojiPicker) return;

        const handleClickOutside = (event: MouseEvent) => {
            if (emojiPickerRef.current && !emojiPickerRef.current.contains(event.target as Node)) {
                setShowEmojiPicker(false);
            }
        };

        document.addEventListener('mousedown', handleClickOutside);
        return () => {
            document.removeEventListener('mousedown', handleClickOutside);
        };
    }, [showEmojiPicker]);

    // Close comments list when clicking outside
    React.useEffect(() => {
        if (!showCommentsList) return;

        const handleClickOutside = (event: MouseEvent) => {
            if (commentsListRef.current && !commentsListRef.current.contains(event.target as Node)) {
                setShowCommentsList(false);
            }
        };

        document.addEventListener('mousedown', handleClickOutside);
        return () => {
            document.removeEventListener('mousedown', handleClickOutside);
        };
    }, [showCommentsList]);

    // Focus the input when comments list opens
    React.useEffect(() => {
        if (showCommentsList && commentInputRef.current) {
            commentInputRef.current.focus();
        }
    }, [showCommentsList]);

    const addComment = () => {
        const newComment: SpanInline = {
            t: 'Span',
            c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: commentText }]]
        };

        const newBlock = structuredClone(block);
        if (newBlock.t === 'Para' || newBlock.t === 'Plain') {
            (newBlock as ParaBlock | PlainBlock).c.push(newComment);
        } else if (newBlock.t === 'Header') {
            (newBlock as HeaderBlock).c[2].push(newComment);
        }
        setLocalAst(newBlock);
        setCommentText('')
    };

    // Reactji-authorship logic (2026-05-25 plan, Phase 2c).
    //
    // Click priority:
    //   1. If we know "me" AND attribution is available, walk
    //      `reactionSpans` for a same-emoji span attributed to me.
    //      If found, remove that span — `setLocalAst` flows through
    //      `incrementalWriteQmd` and the CRDT round-trip restores
    //      `s` on the next render, so the bookkeeping closes itself.
    //   2. Otherwise, fall through to the legacy add-only behaviour.
    //      `me === null` (no auth / no Automerge actor) and a missing
    //      attribution lookup (Attribution toggle off, the opt-in
    //      default per the 2026-05-25 decision log) both land here.
    //
    // Known edge case from the decision log: a fast double-click
    // inside the ~50–150 ms round-trip window can fall through to
    // duplicate-add, since the just-added span doesn't have `s` yet.
    // Acceptable for the demo.
    const renderer = (window as any).__Q2_PREVIEW_RENDERER__;
    const me: string | null = renderer.useCurrentActor();
    const attributionLookup = React.useContext(renderer.AttributionLookupContext);

    const spanEmojiText = (span: InlineNode): string =>
        (span as SpanInline).c[1]
            .map((o: InlineNode) => o.t === 'Str' ? (o as StrInline).c : '')
            .join('');

    const findMineSpan = (emoji: string): InlineNode | null => {
        if (!me || !attributionLookup) return null;
        for (const span of reactionSpans) {
            if (spanEmojiText(span) !== emoji) continue;
            const s = (span as any).s;
            if (s == null) continue;
            const attr = attributionLookup.get(s);
            if (attr?.actor === me) return span;
        }
        return null;
    };

    const removeSpanByS = (sToRemove: number): void => {
        const newBlock: BlockNode = structuredClone(block) as BlockNode;
        const filterChildren = (arr: InlineNode[]) => {
            const idx = arr.findIndex(n => (n as any).s === sToRemove);
            if (idx >= 0) arr.splice(idx, 1);
        };
        if (newBlock.t === 'Para' || newBlock.t === 'Plain') {
            filterChildren((newBlock as ParaBlock | PlainBlock).c);
        } else if (newBlock.t === 'Header') {
            filterChildren((newBlock as HeaderBlock).c[2]);
        }
        setLocalAst(newBlock);
    };

    const addReaction = (emoji: string) => {
        // Phase 1 debug — record the click event for the e2e spec.
        try {
            const diag = (window as any).__COMMENT_DIAG__;
            if (diag) {
                diag.addReactionCalls ??= [];
                diag.addReactionCalls.push({
                    emoji,
                    blockType: block.t,
                    reactionSpansLen: reactionSpans.length,
                    me,
                    attributionLookupNull: !attributionLookup,
                });
            }
        } catch { /* noop */ }

        const mineSpan = findMineSpan(emoji);
        if (mineSpan && (mineSpan as any).s != null) {
            removeSpanByS((mineSpan as any).s);
            setShowEmojiPicker(false);
            return;
        }

        const newReaction: SpanInline = {
            t: 'Span',
            c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: emoji }]]
        };

        const newBlock: BlockNode = structuredClone(block) as BlockNode;
        if (newBlock.t === 'Para' || newBlock.t === 'Plain') {
            (newBlock as ParaBlock | PlainBlock).c.push(newReaction);
        } else if (newBlock.t === 'Header') {
            (newBlock as HeaderBlock).c[2].push(newReaction);
        }
        setLocalAst(newBlock);
        setShowEmojiPicker(false);
    };

    const commonEmojis = ['👍', '❤️', '😂', '🎉', '🤔', '👀', '🔥', '✅'];
    const reactionEntries = Array.from(reactionCounts.entries());
    const hasContent = reactionEntries.length > 0 || comments.length > 0;

    // Phase 1 diagnostic mount-gate. The Phase 2c block above already
    // declared `renderer` — re-use it. The gate stays so the diagnostic
    // doesn't try to call surfaces that aren't there on older builds.
    const diagAvailable =
        typeof renderer?.useNodeAttribution === 'function' &&
        typeof renderer?.useCurrentActor === 'function';

    // Mount Diagnostic on any block that has *either* reactji spans
    // or text comments — both originate from `[>> …]{.quarto-edit-comment}`
    // and both carry the `s` source-info index the attribution lookup
    // needs. Picking the first reactji span (when present) keeps the
    // multi-🤔 H1 fixture as the canonical attribution probe.
    const diagAnchor = reactionSpans[0] ?? comments[0] ?? null;

    return (
        <div style={{
            position: 'relative',
        }}>
            {diagAvailable && diagAnchor ? <Diagnostic first={diagAnchor} /> : null}
            {children}

            {/* Container for all bubbles */}
            <div
                style={{
                    position: 'absolute',
                    bottom: '-11px',
                    right: '-10px',
                    display: 'flex',
                    flexDirection: 'row',
                    gap: '4px',
                    alignItems: 'center',
                    opacity: hasContent || isHovered || showEmojiPicker || showCommentsList ? 1 : 0.2,
                    transition: 'opacity 0.2s',
                }}
                onMouseEnter={() => setIsHovered(true)}
                onMouseLeave={() => setIsHovered(false)}
            >
                {/* Reaction count bubbles. Phase 2d polish (2026-05-25
                    plan): when *any* reactji span for this emoji is
                    attributed to me, render the bubble with my
                    attribution color as the border accent. With
                    Attribution toggle off, `attributionLookup` is null
                    and `mineColorForEmoji` always returns null — the
                    bubble keeps its default neutral grey. */}
                {reactionEntries.map(([emoji, count]) => {
                    const emojiStr = emoji as string;
                    let mineColor: string | null = null;
                    if (me && attributionLookup) {
                        for (const span of reactionSpans) {
                            if (spanEmojiText(span) !== emojiStr) continue;
                            const s = (span as any).s;
                            if (s == null) continue;
                            const attr = attributionLookup.get(s);
                            if (attr?.actor === me) {
                                mineColor = attr.color;
                                break;
                            }
                        }
                    }
                    return (
                    <div
                        key={emojiStr}
                        style={{
                            backgroundColor: '#dbdbdb',
                            color: '#333',
                            padding: '2px 5px',
                            borderRadius: '12px',
                            border: mineColor ? `2px solid ${mineColor}` : '1px solid #777',
                            cursor: 'pointer',
                            boxShadow: '0 2px 4px rgba(0,0,0,0.2)',
                            userSelect: 'none',
                            display: 'flex',
                            gap: '4px',
                            transition: 'background-color 0.2s',
                            fontSize: '0.8rem',
                        }}
                        onClick={() => addReaction(emoji as string)}
                        onMouseEnter={(e) => e.currentTarget.style.backgroundColor = '#ededed'}
                        onMouseLeave={(e) => e.currentTarget.style.backgroundColor = '#dbdbdb'}
                        title={`Add ${emoji}`}
                    >
                        <span>{emoji}</span>
                        <span>{count}</span>
                    </div>
                    );
                })}

                {/* Add reaction bubble */}
                <div
                    ref={emojiPickerRef}
                    style={{
                        position: 'relative',
                    }}>
                    <div
                        style={{
                            backgroundColor: showEmojiPicker ? '#e0f0ff' : '#b3d9ff',
                            color: '#4a7ba7',
                            padding: '2px 5px',
                            borderRadius: '12px',
                            border: '1px solid #4a7ba7',
                            cursor: 'pointer',
                            boxShadow: '0 2px 4px rgba(0,0,0,0.2)',
                            userSelect: 'none',
                            transition: 'background-color 0.2s',
                            fontSize: '0.8rem',
                        }}
                        onClick={() => setShowEmojiPicker(!showEmojiPicker)}
                        onMouseEnter={(e) => e.currentTarget.style.backgroundColor = '#e0f0ff'}
                        onMouseLeave={(e) => e.currentTarget.style.backgroundColor = showEmojiPicker ? '#e0f0ff' : '#b3d9ff'}
                        title="Add reaction"
                    >
                        + 🙂
                    </div>

                    {/* Simple emoji picker */}
                    {showEmojiPicker && (
                        <div style={{
                            position: 'absolute',
                            marginBottom: '4px',
                            top: '30px',
                            backgroundColor: 'white',
                            border: '1px solid #ccc',
                            borderRadius: '8px',
                            padding: '8px',
                            boxShadow: '0 4px 8px rgba(0,0,0,0.2)',
                            display: 'flex',
                            flexDirection: 'row',
                            gap: '4px',
                            right: '0',
                            zIndex: '9999',
                            fontSize: '1rem',
                        }}>
                            {commonEmojis.map(emoji => (
                                <span
                                    key={emoji}
                                    style={{
                                        cursor: 'pointer',
                                        padding: '4px',
                                        borderRadius: '4px',
                                        transition: 'background-color 0.2s',
                                    }}
                                    onClick={() => addReaction(emoji)}
                                    onMouseEnter={(e) => e.currentTarget.style.backgroundColor = '#f0f0f0'}
                                    onMouseLeave={(e) => e.currentTarget.style.backgroundColor = 'transparent'}
                                >
                                    {emoji}
                                </span>
                            ))}
                        </div>
                    )}
                </div>

                {/* Comments count bubble */}
                {(
                    <div
                        ref={commentsListRef}
                        style={{
                            position: 'relative',
                        }}>
                        <div
                            style={{
                                backgroundColor: showCommentsList ? '#e0f0ff' : '#b3d9ff',
                                color: '#4a7ba7',
                                padding: '4px 8px',
                                borderRadius: '12px',
                                border: '1px solid #4a7ba7',
                                fontSize: '0.7rem',
                                cursor: 'pointer',
                                boxShadow: '0 2px 4px rgba(0,0,0,0.2)',
                                userSelect: 'none',
                                transition: 'background-color 0.2s',
                            }}
                            onClick={() => setShowCommentsList(!showCommentsList)}
                            onMouseEnter={(e) => e.currentTarget.style.backgroundColor = '#e0f0ff'}
                            onMouseLeave={(e) => e.currentTarget.style.backgroundColor = showCommentsList ? '#e0f0ff' : '#b3d9ff'}
                            title={`${comments.length} comment${comments.length !== 1 ? 's' : ''}`}
                        >
                            💬 {comments.length}
                        </div>

                        {/* Comments list popup */}
                        {showCommentsList && (
                            <div style={{
                                position: 'absolute',
                                top: '30px',
                                right: '0',
                                backgroundColor: 'white',
                                border: '1px solid #ccc',
                                borderRadius: '8px',
                                padding: '8px',
                                boxShadow: '0 4px 8px rgba(0,0,0,0.2)',
                                minWidth: '200px',
                                maxWidth: '300px',
                                zIndex: '9999'
                            }}>
                                {comments.map((comment, i) => {
                                    const commentContent = (comment as SpanInline).c[1]
                                        .map((inline: InlineNode) => {
                                            if (inline.t === 'Str') return (inline as StrInline).c;
                                            if (inline.t === 'Space') return ' ';
                                            return '';
                                        })
                                        .join('');

                                    return (
                                        <div
                                            key={i}
                                            style={{
                                                padding: '8px',
                                                borderBottom: i < comments.length - 1 ? '1px solid #eee' : 'none',
                                                fontSize: '0.75rem',
                                                color: '#333',
                                                wordWrap: 'break-word'
                                            }}
                                        >
                                            {commentContent}
                                        </div>
                                    );
                                })}
                                <div style={{ marginTop: '8px', display: 'flex', gap: '4px' }}>
                                    <input
                                        ref={commentInputRef}
                                        value={commentText}
                                        onChange={(e) => setCommentText(e.target.value)}
                                        onKeyDown={(e) => e.key === 'Enter' && commentText && addComment()}
                                        placeholder="Add comment"
                                        style={{ flex: 1, padding: '4px', fontFamily: 'monospace', fontSize: '0.75rem', backgroundColor: '#f0f0f0', color: 'black', border: '1px solid #ccc', borderRadius: '4px' }}
                                    />
                                    <button onClick={addComment} disabled={!commentText} style={{ padding: '4px 8px', backgroundColor: '#f0f0f0', color: '#333', border: '1px solid #ccc', borderRadius: '4px', fontSize: '0.875rem' }}>+</button>
                                </div>
                            </div>
                        )}
                    </div>
                )}
            </div>
        </div>
    );
};
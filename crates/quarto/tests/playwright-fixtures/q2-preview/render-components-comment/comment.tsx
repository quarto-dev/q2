const React = window.React;
const {
    Block: B,
    usePreviewEdit,
} = window.__Q2_PREVIEW_RENDERER__;

// Diagnostic export: surfaces renderer-surface probe results to e2e specs.
// Idempotent under multiple module loads. Fields:
//   surfaceKeys       – snapshot of __Q2_PREVIEW_RENDERER__ keys
//   hasUseNodeAttribution / hasUseCurrentActor – surface presence flags
//   me                – value returned by useCurrentActor() (or null)
//   blocks            – per-block attribution diagnostic (one entry per mount)
//   addReactionCalls  – click records for the e2e spec to inspect
const _diagBootstrap = ((window as any).__COMMENT_DIAG__ ??= {
    me: null,
    blocks: [],
    addReactionCalls: [],
});
_diagBootstrap.surfaceKeys = Object.keys((window as any).__Q2_PREVIEW_RENDERER__ ?? {});
_diagBootstrap.hasUseNodeAttribution = typeof (window as any).__Q2_PREVIEW_RENDERER__?.useNodeAttribution === 'function';
_diagBootstrap.hasUseCurrentActor = typeof (window as any).__Q2_PREVIEW_RENDERER__?.useCurrentActor === 'function';

// Conditional diagnostic sub-component: only mounted when both attribution
// hooks are present (conditional mount is the React-safe gate for hooks).
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

const splitEmoji = (string: string) => [...new Intl.Segmenter().segment(string)].map(x => x.segment)

export const Block = (args: NodeArgs<BlockNode>) => {
    const edit = usePreviewEdit();
    const { node: block, onNavigateToDocument, setLocalAst } = args;

    let comments: InlineNode[] = [];
    let newBlock = block;
    if ('c' in block && block.c) {
        if ((block.t === 'Para' || block.t === 'Plain') && Array.isArray(block.c)) {
            comments = block.c.filter(isComment);
            newBlock = structuredClone(block);
            newBlock.c = block.c.filter((n: any) => !isComment(n));
        } else if (block.t === 'Header' && Array.isArray(block.c) && Array.isArray(block.c[2])) {
            comments = block.c[2].filter(isComment);
            newBlock = structuredClone(block);
            //@ts-ignore
            newBlock.c[2] = block.c[2].filter((n: any) => !isComment(n));
        }
    }

    const commentContents = comments.map((c) =>
        (c as SpanInline).c[1].map((o: InlineNode) => {
            if (o.t === 'Str') return (o as StrInline).c;
            if (o.t === 'Space') return ' ';
            return '';
        }).join('')
    );

    const reactionSpans: InlineNode[] = comments.filter((_, i) => splitEmoji(commentContents[i]).length === 1);
    const reactionCounts = commentContents
        .filter(c => splitEmoji(c).length === 1)
        .reduce((acc, emoji) => acc.set(emoji, (acc.get(emoji) || 0) + 1), new Map<string, number>());
    comments = comments.filter((_, i) => splitEmoji(commentContents[i]).length !== 1);

    if (block.t === 'BulletList' || block.t === 'OrderedList') {
        return <B node={block} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst} />;
    }

    return (
        <CommentWrapper
            reactionCounts={reactionCounts}
            reactionSpans={reactionSpans}
            comments={comments}
            block={block}
            edit={edit}
        >
            <B node={newBlock} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst} />
        </CommentWrapper>
    );
};

type EditHandle = ReturnType<typeof usePreviewEdit>;

const CommentWrapper = ({
    children,
    comments,
    reactionSpans,
    reactionCounts,
    block,
    edit,
}: {
    children: React.ReactNode;
    reactionCounts: Map<string, number>;
    reactionSpans: InlineNode[];
    comments: InlineNode[];
    block: BlockNode;
    edit: EditHandle;
}) => {
    const [commentText, setCommentText] = React.useState('');
    const [showEmojiPicker, setShowEmojiPicker] = React.useState(false);
    const [showCommentsList, setShowCommentsList] = React.useState(false);
    const [isHovered, setIsHovered] = React.useState(false);
    const emojiPickerRef = React.useRef<HTMLDivElement>(null);
    const commentsListRef = React.useRef<HTMLDivElement>(null);
    const commentInputRef = React.useRef<HTMLInputElement>(null);


    React.useEffect(() => {
        if (!showEmojiPicker) return;
        const handleClickOutside = (event: MouseEvent) => {
            if (emojiPickerRef.current && !emojiPickerRef.current.contains(event.target as Node)) {
                setShowEmojiPicker(false);
            }
        };
        document.addEventListener('mousedown', handleClickOutside);
        return () => { document.removeEventListener('mousedown', handleClickOutside); };
    }, [showEmojiPicker]);

    React.useEffect(() => {
        if (!showCommentsList) return;
        const handleClickOutside = (event: MouseEvent) => {
            if (commentsListRef.current && !commentsListRef.current.contains(event.target as Node)) {
                setShowCommentsList(false);
            }
        };
        document.addEventListener('mousedown', handleClickOutside);
        return () => { document.removeEventListener('mousedown', handleClickOutside); };
    }, [showCommentsList]);

    React.useEffect(() => {
        if (showCommentsList && commentInputRef.current) {
            commentInputRef.current.focus();
        }
    }, [showCommentsList]);

    // Attribution-aware helpers (Phase 2c).
    const renderer = (window as any).__Q2_PREVIEW_RENDERER__;
    const me: string | null = renderer.useCurrentActor();
    const attributionLookup = React.useContext(renderer.AttributionLookupContext);

    const spanEmojiText = (span: InlineNode): string =>
        (span as SpanInline).c[1].map((o: InlineNode) => o.t === 'Str' ? (o as StrInline).c : '').join('');

    // Find the first span of this emoji attributed to the current actor.
    // Returns null when attribution is unavailable (toggle off or no auth).
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

    // Remove the first span whose emoji text matches from the source node.
    const removeFirstMatchingInSource = (emoji: string): void => {
        const resolved = edit.resolveSource(block);
        if (!resolved) return;
        const modified = structuredClone(resolved.sourceNode);
        const filterOut = (arr: InlineNode[]) => {
            const idx = arr.findIndex(n =>
                isComment(n) && splitEmoji(spanEmojiText(n)).length === 1 && spanEmojiText(n) === emoji
            );
            if (idx >= 0) arr.splice(idx, 1);
        };
        if (modified.t === 'Para' || modified.t === 'Plain') {
            filterOut((modified as ParaBlock | PlainBlock).c);
        } else if (modified.t === 'Header') {
            filterOut((modified as HeaderBlock).c[2]);
        }
        edit.commitSubtreeEdit(JSON.stringify(resolved.sourceEntry), modified);
    };

    // Append an inline span to the source node and commit.
    const appendInlineToSource = (newSpan: SpanInline): void => {
        const resolved = edit.resolveSource(block);
        if (!resolved) return;
        const modified = structuredClone(resolved.sourceNode);
        if (modified.t === 'Para' || modified.t === 'Plain') {
            (modified as ParaBlock | PlainBlock).c.push(newSpan);
        } else if (modified.t === 'Header') {
            (modified as HeaderBlock).c[2].push(newSpan);
        }
        edit.commitSubtreeEdit(JSON.stringify(resolved.sourceEntry), modified);
    };

    const addComment = () => {
        const newComment: SpanInline = {
            t: 'Span',
            c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: commentText }]],
        };
        appendInlineToSource(newComment);
        setCommentText('');
    };

    const addReaction = (emoji: string) => {
        // Record for e2e diagnostic.
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

        // Remove path: attribution identifies the current actor's span.
        if (findMineSpan(emoji)) {
            removeFirstMatchingInSource(emoji);
            setShowEmojiPicker(false);
            return;
        }

        // Add path.
        const newReaction: SpanInline = {
            t: 'Span',
            c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: emoji }]],
        };
        appendInlineToSource(newReaction);
        setShowEmojiPicker(false);
    };

    const diagAvailable =
        typeof renderer?.useNodeAttribution === 'function' &&
        typeof renderer?.useCurrentActor === 'function';
    const diagAnchor = reactionSpans[0] ?? comments[0] ?? null;

    const commonEmojis = ['👍', '❤️', '😂', '🎉', '🤔', '👀', '🔥', '✅'];
    const reactionEntries = Array.from(reactionCounts.entries());
    const hasContent = reactionEntries.length > 0 || comments.length > 0;

    const mineColorForEmoji = (emojiStr: string): string | null => {
        if (!me || !attributionLookup) return null;
        for (const span of reactionSpans) {
            if (spanEmojiText(span) !== emojiStr) continue;
            const s = (span as any).s;
            if (s == null) continue;
            const attr = attributionLookup.get(s);
            if (attr?.actor === me) return attr.color;
        }
        return null;
    };

    return (
        <div style={{ position: 'relative' }}>
            {diagAvailable && diagAnchor ? <Diagnostic first={diagAnchor} /> : null}
            {children}

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
                {reactionEntries.map(([emoji, count]) => {
                    const emojiStr = emoji as string;
                    const mineColor = mineColorForEmoji(emojiStr);
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
                            onClick={() => addReaction(emojiStr)}
                            onMouseEnter={(e) => e.currentTarget.style.backgroundColor = '#ededed'}
                            onMouseLeave={(e) => e.currentTarget.style.backgroundColor = '#dbdbdb'}
                            title={`Add ${emoji}`}
                        >
                            <span>{emoji}</span>
                            <span>{count}</span>
                        </div>
                    );
                })}

                <div ref={emojiPickerRef} style={{ position: 'relative' }}>
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
                    {showEmojiPicker && (
                        <div style={{
                            position: 'absolute',
                            top: '30px',
                            right: '0',
                            backgroundColor: 'white',
                            border: '1px solid #ccc',
                            borderRadius: '8px',
                            padding: '8px',
                            boxShadow: '0 4px 8px rgba(0,0,0,0.2)',
                            display: 'flex',
                            flexDirection: 'row',
                            gap: '4px',
                            zIndex: '9999',
                            fontSize: '1rem',
                        }}>
                            {commonEmojis.map(emoji => (
                                <span
                                    key={emoji}
                                    style={{ cursor: 'pointer', padding: '4px', borderRadius: '4px' }}
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

                <div ref={commentsListRef} style={{ position: 'relative' }}>
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
                        }}
                        onClick={() => setShowCommentsList(!showCommentsList)}
                        title={`${comments.length} comment${comments.length !== 1 ? 's' : ''}`}
                    >
                        💬 {comments.length}
                    </div>
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
                            zIndex: '9999',
                        }}>
                            {comments.map((comment, i) => {
                                const text = (comment as SpanInline).c[1]
                                    .map((inline: InlineNode) => {
                                        if (inline.t === 'Str') return (inline as StrInline).c;
                                        if (inline.t === 'Space') return ' ';
                                        return '';
                                    }).join('');
                                return (
                                    <div key={i} style={{
                                        padding: '8px',
                                        borderBottom: i < comments.length - 1 ? '1px solid #eee' : 'none',
                                        fontSize: '0.75rem',
                                        color: '#333',
                                        wordWrap: 'break-word',
                                    }}>
                                        {text}
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
            </div>
        </div>
    );
};

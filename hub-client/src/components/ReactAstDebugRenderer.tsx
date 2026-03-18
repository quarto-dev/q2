import React from 'react';

/**
 * Simplified Pandoc AST types for rendering
 */
interface PandocAST {
    'pandoc-api-version': [number, number, number];
    meta: Record<string, unknown>;
    blocks: Block[];
}

type ParaBlock = { t: 'Para'; c: Inline[] };
type PlainBlock = { t: 'Plain'; c: Inline[] };
type HeaderBlock = { t: 'Header'; c: [number, [string, string[], [string, string][]], Inline[]] };
type CodeBlock = { t: 'CodeBlock'; c: [[string, string[], [string, string][]], string] };
type BulletListBlock = { t: 'BulletList'; c: Block[][] };
type OrderedListBlock = { t: 'OrderedList'; c: [[number, { t: string }, { t: string }], Block[][]] };
type BlockQuoteBlock = { t: 'BlockQuote'; c: Block[] };
type DivBlock = { t: 'Div'; c: [[string, string[], [string, string][]], Block[]] };
type HorizontalRuleBlock = { t: 'HorizontalRule' };
type RawBlock = { t: 'RawBlock'; c: [string, string] };
type FigureBlock = { t: 'Figure'; c: [[string, string[], [string, string][]], [Inline[] | null, Block[]], Block[]] };
type UnknownBlock = { t: string; c?: unknown };

type Block =
    | ParaBlock
    | PlainBlock
    | HeaderBlock
    | CodeBlock
    | BulletListBlock
    | OrderedListBlock
    | BlockQuoteBlock
    | DivBlock
    | HorizontalRuleBlock
    | RawBlock
    | FigureBlock
    | UnknownBlock;

type StrInline = { t: 'Str'; c: string };
type SpaceInline = { t: 'Space' };
type SoftBreakInline = { t: 'SoftBreak' };
type LineBreakInline = { t: 'LineBreak' };
type EmphInline = { t: 'Emph'; c: Inline[] };
type StrongInline = { t: 'Strong'; c: Inline[] };
type CodeInline = { t: 'Code'; c: [[string, string[], [string, string][]], string] };
type LinkInline = { t: 'Link'; c: [[string, string[], [string, string][]], Inline[], [string, string]] };
type ImageInline = { t: 'Image'; c: [[string, string[], [string, string][]], Inline[], [string, string]] };
type SpanInline = { t: 'Span'; c: [[string, string[], [string, string][]], Inline[]] };
type UnknownInline = { t: string; c?: unknown };

type Inline =
    | StrInline
    | SpaceInline
    | SoftBreakInline
    | LineBreakInline
    | EmphInline
    | StrongInline
    | CodeInline
    | LinkInline
    | ImageInline
    | SpanInline
    | UnknownInline;

interface PandocAstRendererProps {
    astJson: string;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
}

/**
 * Component that renders Pandoc AST in debug mode (uniform structure)
 */
export function Ast({ astJson, onNavigateToDocument, setAst }: PandocAstRendererProps) {
    let ast: PandocAST;

    try {
        ast = JSON.parse(astJson);
    } catch (err) {
        return (
            <div className="error" style={{ padding: '20px', color: 'red' }}>
                Failed to parse AST: {err instanceof Error ? err.message : String(err)}
            </div>
        );
    }

    return (
        <div className="pandoc-content-debug" style={{ padding: '20px', fontFamily: 'monospace', fontSize: '12px' }}>
            {ast.blocks.map((block, i) =>
                <Block
                    key={i}
                    block={block}
                    onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={(newBlock: Block) => {
                        const newBlocks = [...ast.blocks];
                        newBlocks[i] = newBlock;
                        setAst({ ...ast, blocks: newBlocks });
                    }}
                />)
            }
        </div>
    );
}

/**
 * Helper to create a setLocalAst function for a block at a given index
 */
function createBlockSetLocalAst(
    index: number,
    blocks: Block[],
    setParentAst: (newBlock: Block) => void,
    recreateParent: (newBlocks: Block[]) => Block
): (newBlock: Block) => void {
    return (newBlock: Block) => {
        const newBlocks = [...blocks];
        newBlocks[index] = newBlock;
        setParentAst(recreateParent(newBlocks));
    };
}

/**
 * Helper to create a setLocalAst function for an inline at a given index
 */
function createInlineSetLocalAst(
    index: number,
    inlines: Inline[],
    setParentAst: (newInline: Inline) => void,
    recreateParent: (newInlines: Inline[]) => Inline
): (newInline: Inline) => void {
    return (newInline: Inline) => {
        const newInlines = [...inlines];
        newInlines[index] = newInline;
        setParentAst(recreateParent(newInlines));
    };
}

/**
 * Helper to create a setLocalAst function for a block in a nested list (Block[][])
 */
function createNestedBlockSetLocalAst(
    itemIndex: number,
    blockIndex: number,
    nestedBlocks: Block[][],
    setParentAst: (newBlock: Block) => void,
    recreateParent: (newNestedBlocks: Block[][]) => Block
): (newBlock: Block) => void {
    return (newBlock: Block) => {
        const newItems = [...nestedBlocks];
        const newItem = [...newItems[itemIndex]];
        newItem[blockIndex] = newBlock;
        newItems[itemIndex] = newItem;
        setParentAst(recreateParent(newItems));
    };
}

// Uniform styling for all blocks
const blockStyle: React.CSSProperties = {
    border: '1px solid #666',
    padding: '4px',
    margin: '4px 0',
    backgroundColor: '#f5f5f5',
    position: 'relative',
};

// Uniform styling for all inlines
const inlineStyle: React.CSSProperties = {
    border: '1px solid #999',
    padding: '2px',
    margin: '1px',
    backgroundColor: '#e8e8e8',
    display: 'inline-block'
};

const Para = ({ c, onNavigateToDocument, setLocalAst }: { c: Inline[], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newBlock: Block) => void }) => (
    <div style={blockStyle}>
        <strong>Para:</strong> {c.map((inline, i) => (
            <Inline
                key={i}
                inline={inline}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createInlineSetLocalAst(i, c, setLocalAst, (newInlines) => ({ t: 'Para', c: newInlines }))}
            />
        ))}
    </div>
);

const Plain = ({ c, onNavigateToDocument, setLocalAst }: { c: Inline[], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newBlock: Block) => void }) => (
    <div style={blockStyle}>
        <strong>Plain:</strong> {c.map((inline, i) => (
            <Inline
                key={i}
                inline={inline}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createInlineSetLocalAst(i, c, setLocalAst, (newInlines) => ({ t: 'Plain', c: newInlines }))}
            />
        ))}
    </div>
);

const Header = ({ c, onNavigateToDocument, setLocalAst }: { c: [number, [string, string[], [string, string][]], Inline[]], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newBlock: Block) => void }) => (
    <div style={blockStyle}>
        <strong>Header(level={c[0]}):</strong> {c[2].map((inline, i) => (
            <Inline
                key={i}
                inline={inline}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createInlineSetLocalAst(i, c[2], setLocalAst, (newInlines) => ({ t: 'Header', c: [c[0], c[1], newInlines] }))}
            />
        ))}
    </div>
);

const CodeBlock = ({ c }: { c: [[string, string[], [string, string][]], string], setLocalAst: (newBlock: Block) => void }) => (
    <div style={blockStyle}>
        <strong>CodeBlock:</strong> <code>{c[1]}</code>
    </div>
);

const BulletList = ({ c, onNavigateToDocument, setLocalAst }: { c: Block[][], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newBlock: Block) => void }) => (
    <div style={blockStyle}>
        <strong>BulletList:</strong>
        {c.map((item, i) => (
            <div key={i} style={{ marginLeft: '20px' }}>
                {item.map((block, j) => (
                    <Block
                        key={j}
                        block={block}
                        onNavigateToDocument={onNavigateToDocument}
                        setLocalAst={createNestedBlockSetLocalAst(i, j, c, setLocalAst, (newItems) => ({ t: 'BulletList', c: newItems }))}
                    />
                ))}
            </div>
        ))}
    </div>
);

const OrderedList = ({ c, onNavigateToDocument, setLocalAst }: { c: [[number, { t: string }, { t: string }], Block[][]], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newBlock: Block) => void }) => (
    <div style={blockStyle}>
        <strong>OrderedList(start={c[0][0]}):</strong>
        {c[1].map((item, i) => (
            <div key={i} style={{ marginLeft: '20px' }}>
                {item.map((block, j) => (
                    <Block
                        key={j}
                        block={block}
                        onNavigateToDocument={onNavigateToDocument}
                        setLocalAst={createNestedBlockSetLocalAst(i, j, c[1], setLocalAst, (newItems) => ({ t: 'OrderedList', c: [c[0], newItems] }))}
                    />
                ))}
            </div>
        ))}
    </div>
);

const BlockQuote = ({ c, onNavigateToDocument, setLocalAst }: { c: Block[], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newBlock: Block) => void }) => (
    <div style={blockStyle}>
        <strong>BlockQuote:</strong>
        {c.map((block, i) => (
            <Block
                key={i}
                block={block}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createBlockSetLocalAst(i, c, setLocalAst, (newBlocks) => ({ t: 'BlockQuote', c: newBlocks }))}
            />
        ))}
    </div>
);

const Div = ({ c, onNavigateToDocument, setLocalAst }: { c: [[string, string[], [string, string][]], Block[]], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newBlock: Block) => void }) => (
    <div style={blockStyle}>
        <strong>Div:</strong>
        {c[1].map((block, i) => (
            <Block
                key={i}
                block={block}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createBlockSetLocalAst(i, c[1], setLocalAst, (newBlocks) => ({ t: 'Div', c: [c[0], newBlocks] }))}
            />
        ))}
    </div>
);

const HorizontalRule = ({}: { setLocalAst: (newBlock: Block) => void }) => (
    <div style={blockStyle}>
        <strong>HorizontalRule</strong>
    </div>
);

const RawBlock = ({ c }: { c: [string, string], setLocalAst: (newBlock: Block) => void }) => (
    <div style={blockStyle}>
        <strong>RawBlock({c[0]}):</strong> {c[1]}
    </div>
);

const Figure = ({ c, onNavigateToDocument, setLocalAst }: { c: [[string, string[], [string, string][]], [Inline[] | null, Block[]], Block[]], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newBlock: Block) => void }) => (
    <div style={blockStyle}>
        <strong>Figure:</strong>
        {c[2].map((block, i) => (
            <Block
                key={i}
                block={block}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createBlockSetLocalAst(i, c[2], setLocalAst, (newBlocks) => ({ t: 'Figure', c: [c[0], c[1], newBlocks] }))}
            />
        ))}
        {c[1][0] && <div><em>Caption:</em> {c[1][0].map((inline, i) => (
            <Inline
                key={i}
                inline={inline}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createInlineSetLocalAst(i, c[1][0]!, setLocalAst, (newCaption) => ({ t: 'Figure', c: [c[0], [newCaption, c[1][1]], c[2]] }))}
            />
        ))}</div>}
    </div>
);

const BlockRegistry: Record<string, (props: any) => React.ReactNode> = {
    Para,
    Plain,
    Header,
    CodeBlock,
    BulletList,
    OrderedList,
    BlockQuote,
    Div,
    HorizontalRule,
    RawBlock,
    Figure,
};

const splitEmoji = (string: string) => [...new Intl.Segmenter().segment(string)].map(x => x.segment)
const Block = ({ block, onNavigateToDocument, setLocalAst }: { block: Block, onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newBlock: Block) => void }) => {
    // Gather comments from inline children if block has them
    let comments: Inline[] = [];
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

    const commentContents = comments.map((c) => (c as SpanInline).c[1].map((o: Inline) => {
        if (o.t === 'Str') return (o as StrInline).c;
        if (o.t === 'Space') return ' ';
        return '';
    }).join(''))
    const reactions = commentContents.filter(c => splitEmoji(c).length === 1)
    const reactionCounts = reactions.reduce((acc, emoji) =>
        acc.set(emoji, (acc.get(emoji) || 0) + 1),
        new Map<string, number>()
    );
    comments = comments.filter((_, i) => splitEmoji(commentContents[i]).length !== 1)

    const Component = BlockRegistry[newBlock.t];
    const content = Component ? <Component {...newBlock} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst} /> : <div style={blockStyle}><strong>Not registered: {newBlock.t}</strong></div>;

    // Skip CommentWrapper for BulletList and OrderedList
    if (block.t === 'BulletList' || block.t === 'OrderedList') {
        return <>{content}</>;
    }

    return <CommentWrapper reactionCounts={reactionCounts} comments={comments} setLocalAst={setLocalAst} block={block}>{content}</CommentWrapper>;
};
type StrNodeArgs = { c: string, setLocalAst: (newInline: Inline) => void }

const Str = ({ c, setLocalAst }: StrNodeArgs) => (
    <span style={inlineStyle} onClick={() => {
        setLocalAst({ t: 'Str', c: c + '🔥' })
    }}><strong>Str:</strong> {c}</span>
);

const Space = ({}: { setLocalAst: (newInline: Inline) => void }) => (
    <span style={inlineStyle}><strong>Space</strong></span>
);

const SoftBreak = ({}: { setLocalAst: (newInline: Inline) => void }) => (
    <span style={inlineStyle}><strong>SoftBreak</strong></span>
);

const LineBreak = ({}: { setLocalAst: (newInline: Inline) => void }) => (
    <span style={inlineStyle}><strong>LineBreak</strong></span>
);

const Emph = ({ c, onNavigateToDocument, setLocalAst }: { c: Inline[], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newInline: Inline) => void }) => (
    <span style={inlineStyle}>
        <strong>Emph:</strong> {c.map((inline, i) => (
            <Inline
                key={i}
                inline={inline}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createInlineSetLocalAst(i, c, setLocalAst, (newInlines) => ({ t: 'Emph', c: newInlines }))}
            />
        ))}
    </span>
);

const Strong = ({ c, onNavigateToDocument, setLocalAst }: { c: Inline[], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newInline: Inline) => void }) => (
    <span style={inlineStyle}>
        <strong>Strong:</strong> {c.map((inline, i) => (
            <Inline
                key={i}
                inline={inline}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createInlineSetLocalAst(i, c, setLocalAst, (newInlines) => ({ t: 'Strong', c: newInlines }))}
            />
        ))}
    </span>
);

const Code = ({ c }: { c: [[string, string[], [string, string][]], string], setLocalAst: (newInline: Inline) => void }) => (
    <span style={inlineStyle}><strong>Code:</strong> {c[1]}</span>
);

const Link = ({ c, onNavigateToDocument, setLocalAst }: { c: [[string, string[], [string, string][]], Inline[], [string, string]], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newInline: Inline) => void }) => (
    <span style={inlineStyle}>
        <strong>Link({c[2][0]}):</strong> {c[1].map((inline, i) => (
            <Inline
                key={i}
                inline={inline}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createInlineSetLocalAst(i, c[1], setLocalAst, (newInlines) => ({ t: 'Link', c: [c[0], newInlines, c[2]] }))}
            />
        ))}
    </span>
);

const Image = ({ c, onNavigateToDocument, setLocalAst }: { c: [[string, string[], [string, string][]], Inline[], [string, string]], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newInline: Inline) => void }) => (
    <span style={inlineStyle}>
        <strong>Image({c[2][0]}):</strong> {c[1].map((inline, i) => (
            <Inline
                key={i}
                inline={inline}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createInlineSetLocalAst(i, c[1], setLocalAst, (newInlines) => ({ t: 'Image', c: [c[0], newInlines, c[2]] }))}
            />
        ))}
    </span>
);

const Span = ({ c, onNavigateToDocument, setLocalAst }: { c: [[string, string[], [string, string][]], Inline[]], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newInline: Inline) => void }) => (
    <span style={inlineStyle}>
        <strong>Span:</strong> {c[1].map((inline, i) => (
            <Inline
                key={i}
                inline={inline}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createInlineSetLocalAst(i, c[1], setLocalAst, (newInlines) => ({ t: 'Span', c: [c[0], newInlines] }))}
            />
        ))}
    </span>
);

const Comment = ({ c, onNavigateToDocument, setLocalAst }: { c: [[string, string[], [string, string][]], Inline[]], onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newInline: Inline) => void }) => (
    <span style={{ ...inlineStyle, backgroundColor: '#fff3cd', borderColor: '#ffc107' }}>
        <strong>Comment:</strong> {c[1].map((inline, i) => (
            <Inline
                key={i}
                inline={inline}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={createInlineSetLocalAst(i, c[1], setLocalAst, (newInlines) => ({ t: 'Span', c: [c[0], newInlines] }))}
            />
        ))}
    </span>
);

const InlineRegistry: Record<string, (props: any) => React.ReactNode> = {
    Str,
    Space,
    SoftBreak,
    LineBreak,
    Emph,
    Strong,
    Code,
    Link,
    Image,
    Span,
    Comment,
};

/**
 * Check if an inline is a comment
 */
function isComment(inline: Inline): boolean {
    if (inline.t === 'Span' && 'c' in inline) {
        const attrs = (inline as SpanInline).c[0];
        const classes = attrs[1];
        return classes.includes('quarto-edit-comment');
    }
    return false;
}

/**
 * CommentWrapper renders children in a box and displays gathered comments
 */
const CommentWrapper = ({ children, comments, reactionCounts, setLocalAst, block }: { children: React.ReactNode, reactionCounts: Map<String, number>, comments: Inline[], setLocalAst: (newBlock: Block) => void, block: Block }) => {
    const [commentText, setCommentText] = React.useState('');
    const [showEmojiPicker, setShowEmojiPicker] = React.useState(false);
    const [showCommentsList, setShowCommentsList] = React.useState(false);
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

        const newBlock: Block = structuredClone(block) as Block;
        if (newBlock.t === 'Para' || newBlock.t === 'Plain') {
            (newBlock as ParaBlock | PlainBlock).c.push(newComment);
        } else if (newBlock.t === 'Header') {
            (newBlock as HeaderBlock).c[2].push(newComment);
        }
        setLocalAst(newBlock);
        setCommentText('')
    };

    const addReaction = (emoji: string) => {
        const newReaction: SpanInline = {
            t: 'Span',
            c: [['', ['quarto-edit-comment'], []], [{ t: 'Str', c: emoji }]]
        };

        const newBlock: Block = structuredClone(block) as Block;
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

    return (
        <div style={{
            position: 'relative',
        }}>
            {children}

            {/* Container for all bubbles */}
            <div style={{
                position: 'absolute',
                bottom: '-11px',
                right: '-10px',
                display: 'flex',
                flexDirection: 'row',
                gap: '4px',
                alignItems: 'center',
            }}>
                {/* Reaction count bubbles */}
                {reactionEntries.map(([emoji, count]) => (
                    <div
                        key={emoji as string}
                        style={{
                            backgroundColor: '#dbdbdb',
                            color: '#333',
                            padding: '2px 5px',
                            borderRadius: '12px',
                            border: '1px solid #777',
                            cursor: 'pointer',
                            boxShadow: '0 2px 4px rgba(0,0,0,0.2)',
                            userSelect: 'none',
                            display: 'flex',
                            gap: '4px',
                            transition: 'background-color 0.2s',
                        }}
                        onClick={() => addReaction(emoji as string)}
                        onMouseEnter={(e) => e.currentTarget.style.backgroundColor = '#ededed'}
                        onMouseLeave={(e) => e.currentTarget.style.backgroundColor = '#dbdbdb'}
                        title={`Add ${emoji}`}
                    >
                        <span>{emoji}</span>
                        <span>{count}</span>
                    </div>
                ))}

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
                            zIndex: '9999'
                        }}>
                            {commonEmojis.map(emoji => (
                                <span
                                    key={emoji}
                                    style={{
                                        cursor: 'pointer',
                                        padding: '4px',
                                        borderRadius: '4px',
                                        transition: 'background-color 0.2s'
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
                                fontSize: '11px',
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
                                        .map((inline: Inline) => {
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
                                                fontSize: '12px',
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
                                        style={{ flex: 1, padding: '4px', fontFamily: 'monospace', fontSize: '12px', backgroundColor: '#f0f0f0', color: 'black', border: '1px solid #ccc', borderRadius: '4px' }}
                                    />
                                    <button onClick={addComment} disabled={!commentText} style={{ padding: '4px 8px', backgroundColor: '#f0f0f0', color: '#333', border: '1px solid #ccc', borderRadius: '4px' }}>+</button>
                                </div>
                            </div>
                        )}
                    </div>
                )}
            </div>
        </div>
    );
};

const Inline = ({ inline, onNavigateToDocument, setLocalAst }: { inline: Inline, onNavigateToDocument?: (path: string, anchor: string | null) => void, setLocalAst: (newInline: Inline) => void }) => {
    const Component = InlineRegistry[isComment(inline) ? 'Comment' : inline.t];
    return <span onClick={() => console.log(inline)}>
        {Component ?
            <Component {...inline} onNavigateToDocument={onNavigateToDocument} setLocalAst={setLocalAst} /> :
            <span style={inlineStyle}><strong>Not registered: {inline.t}</strong></span>}
    </span>;
};

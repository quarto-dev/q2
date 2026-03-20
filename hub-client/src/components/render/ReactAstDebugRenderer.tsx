import React, { createContext, useContext } from 'react';

// Context for unified component registry
const RegistryContext = createContext<{
    registry: Record<string, (props: any) => React.ReactNode>;
} | null>(null);

/**
 * Simplified Pandoc AST types for rendering
 */
export interface PandocAST {
    'pandoc-api-version': [number, number, number];
    meta: Record<string, unknown>;
    blocks: BlockNode[];
}

export type ParaBlock = { t: 'Para'; c: InlineNode[] };
export type PlainBlock = { t: 'Plain'; c: InlineNode[] };
export type HeaderBlock = { t: 'Header'; c: [number, [string, string[], [string, string][]], InlineNode[]] };
export type CodeBlock = { t: 'CodeBlock'; c: [[string, string[], [string, string][]], string] };
export type BulletListBlock = { t: 'BulletList'; c: BlockNode[][] };
export type OrderedListBlock = { t: 'OrderedList'; c: [[number, { t: string }, { t: string }], BlockNode[][]] };
export type BlockQuoteBlock = { t: 'BlockQuote'; c: BlockNode[] };
export type DivBlock = { t: 'Div'; c: [[string, string[], [string, string][]], BlockNode[]] };
export type HorizontalRuleBlock = { t: 'HorizontalRule' };
export type RawBlock = { t: 'RawBlock'; c: [string, string] };
export type FigureBlock = { t: 'Figure'; c: [[string, string[], [string, string][]], [InlineNode[] | null, BlockNode[]], BlockNode[]] };
export type UnknownBlock = { t: string; c?: unknown };

export type BlockNode =
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

export type StrInline = { t: 'Str'; c: string };
export type SpaceInline = { t: 'Space' };
export type SoftBreakInline = { t: 'SoftBreak' };
export type LineBreakInline = { t: 'LineBreak' };
export type EmphInline = { t: 'Emph'; c: InlineNode[] };
export type StrongInline = { t: 'Strong'; c: InlineNode[] };
export type CodeInline = { t: 'Code'; c: [[string, string[], [string, string][]], string] };
export type LinkInline = { t: 'Link'; c: [[string, string[], [string, string][]], InlineNode[], [string, string]] };
export type ImageInline = { t: 'Image'; c: [[string, string[], [string, string][]], InlineNode[], [string, string]] };
export type SpanInline = { t: 'Span'; c: [[string, string[], [string, string][]], InlineNode[]] };
export type UnknownInline = { t: string; c?: unknown };

export type InlineNode =
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
    registry?: Record<string, (props: any) => React.ReactNode>;
}

export type NodeArgs<T extends BlockNode | InlineNode> = {
    node: T,
    onNavigateToDocument?: (path: string, anchor: string | null) => void,
    setLocalAst: (newNode: BlockNode | InlineNode) => void
}

/**
 * Component that renders Pandoc AST in debug mode (uniform structure)
 */
export function Ast({ astJson, onNavigateToDocument, setAst, registry = componentRegistry }: PandocAstRendererProps) {
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
        <RegistryContext.Provider value={{ registry }}>
            <div className="pandoc-content-debug" style={{ padding: '20px', fontFamily: 'monospace', fontSize: '12px' }}>
                {ast.blocks.map((block, i) =>
                    <Node
                        key={i}
                        node={block}
                        onNavigateToDocument={onNavigateToDocument}
                        setLocalAst={(newBlock: BlockNode) => {
                            const newBlocks = [...ast.blocks];
                            newBlocks[i] = newBlock;
                            setAst({ ...ast, blocks: newBlocks });
                        }}
                    />)
                }
            </div>
        </RegistryContext.Provider>
    );
}

/**
 * Registry of render functions for all node types with children
 */
const renderChildrenRegistry: Record<string, (args: {
    node: any;
    setLocalAst: (newNode: any) => void;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}) => React.ReactNode> = {
    // Inline types
    Emph: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as EmphInline).c.map((child, i) => (
            <Node key={i} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as EmphInline).c];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Emph', c: newChildren });
                }}
            />
        )),
    Strong: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as StrongInline).c.map((child, i) => (
            <Node key={i} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as StrongInline).c];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Strong', c: newChildren });
                }}
            />
        )),
    Link: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as LinkInline).c[1].map((child, i) => (
            <Node key={i} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as LinkInline).c[1]];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Link', c: [(node as LinkInline).c[0], newChildren, (node as LinkInline).c[2]] });
                }}
            />
        )),
    Image: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as ImageInline).c[1].map((child, i) => (
            <Node key={i} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as ImageInline).c[1]];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Image', c: [(node as ImageInline).c[0], newChildren, (node as ImageInline).c[2]] });
                }}
            />
        )),
    Span: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as SpanInline).c[1].map((child, i) => (
            <Node key={i} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as SpanInline).c[1]];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Span', c: [(node as SpanInline).c[0], newChildren] });
                }}
            />
        )),
    // Block types
    Para: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as ParaBlock).c.map((child, i) => (
            <Node key={i} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as ParaBlock).c];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Para', c: newChildren });
                }}
            />
        )),
    Plain: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as PlainBlock).c.map((child, i) => (
            <Node key={i} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as PlainBlock).c];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Plain', c: newChildren });
                }}
            />
        )),
    Header: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as HeaderBlock).c[2].map((child, i) => (
            <Node key={i} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as HeaderBlock).c[2]];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Header', c: [(node as HeaderBlock).c[0], (node as HeaderBlock).c[1], newChildren] });
                }}
            />
        )),
    BlockQuote: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as BlockQuoteBlock).c.map((child, i) => (
            <Node key={i} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as BlockQuoteBlock).c];
                    newChildren[i] = newChild as BlockNode;
                    setLocalAst({ t: 'BlockQuote', c: newChildren });
                }}
            />
        )),
    Div: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as DivBlock).c[1].map((child, i) => (
            <Node key={i} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as DivBlock).c[1]];
                    newChildren[i] = newChild as BlockNode;
                    setLocalAst({ t: 'Div', c: [(node as DivBlock).c[0], newChildren] });
                }}
            />
        )),
    BulletList: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as BulletListBlock).c.map((item, i) => (
            <>{item.map((block, j) => (
                <Node key={JSON.stringify([i, j])} node={block} onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={(newBlock: BlockNode | InlineNode) => {
                        const newItems = [...(node as BulletListBlock).c];
                        const newItem = [...newItems[i]];
                        newItem[j] = newBlock as BlockNode;
                        newItems[i] = newItem;
                        setLocalAst({ t: 'BulletList', c: newItems });
                    }}
                />
            ))}</>
        )),
    OrderedList: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as OrderedListBlock).c[1].map((item, i) => (
            <>{item.map((block, j) => (
                <Node key={JSON.stringify([i, j])} node={block} onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={(newBlock: BlockNode | InlineNode) => {
                        const newItems = [...(node as OrderedListBlock).c[1]];
                        const newItem = [...newItems[i]];
                        newItem[j] = newBlock as BlockNode;
                        newItems[i] = newItem;
                        setLocalAst({ t: 'OrderedList', c: [(node as OrderedListBlock).c[0], newItems] });
                    }}
                />
            ))}
            </>
        )),
    Figure: ({ node, setLocalAst, onNavigateToDocument }) => (
        <>
            {(node as FigureBlock).c[2].map((child, i) => (
                <Node key={i} node={child} onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={(newChild: BlockNode | InlineNode) => {
                        const newChildren = [...(node as FigureBlock).c[2]];
                        newChildren[i] = newChild as BlockNode;
                        setLocalAst({ t: 'Figure', c: [(node as FigureBlock).c[0], (node as FigureBlock).c[1], newChildren] });
                    }}
                />
            ))}
            // TODO: doesn't totally make sense to have this here:
            {(node as FigureBlock).c[1][0] && <div><em>Caption:</em> {(node as FigureBlock).c[1][0]!.map((inline, i) => (
                <Node key={i} node={inline} onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={(newInline: BlockNode | InlineNode) => {
                        const newCaption = [...(node as FigureBlock).c[1][0]!];
                        newCaption[i] = newInline as InlineNode;
                        setLocalAst({ t: 'Figure', c: [(node as FigureBlock).c[0], [newCaption, (node as FigureBlock).c[1][1]], (node as FigureBlock).c[2]] });
                    }}
                />
            ))}</div>}
        </>
    ),
};

/**
 * Unified function to render children of any node type
 */
export function renderChildren<T extends BlockNode | InlineNode>({
    node,
    setLocalAst,
    onNavigateToDocument,
}: {
    node: T;
    setLocalAst: (newNode: T) => void;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}): React.ReactNode {
    const renderer = renderChildrenRegistry[node.t];
    if (!renderer) {
        console.warn(`No renderer found for node type: ${node.t}`);
        return null;
    }

    return renderer({ node, setLocalAst, onNavigateToDocument });
}

export const renderNode = (args: NodeArgs<BlockNode | InlineNode>, type: string) => {
    const registries = useContext(RegistryContext);
    const registry = registries?.registry ?? componentRegistry;

    const Component = registry[type];

    return Component ? <Component {...args} /> : <div style={blockStyle}><strong>Not registered: {args.node.t}</strong></div>
}


// Uniform styling for all blocks
export const blockStyle: React.CSSProperties = {
    border: '1px solid #666',
    padding: '4px',
    margin: '4px 0',
    backgroundColor: '#f5f5f5',
    position: 'relative',
};

// Uniform styling for all inlines
export const inlineStyle: React.CSSProperties = {
    border: '1px solid #999',
    padding: '2px',
    margin: '1px',
    backgroundColor: '#e8e8e8',
    display: 'inline-block'
};

const Para = (args: NodeArgs<ParaBlock>) => (
    <div style={blockStyle}>
        <strong>Para:</strong> {renderChildren(args)}
    </div>
);

const Plain = (args: NodeArgs<PlainBlock>) => (
    <div style={blockStyle}>
        <strong>Plain:</strong> {renderChildren(args)}
    </div>
);

const Header = (args: NodeArgs<HeaderBlock>) => (
    <div style={blockStyle}>
        <strong>Header(level={args.node.c[0]}):</strong> {renderChildren(args)}
    </div>
);

const CodeBlock = (args: NodeArgs<CodeBlock>) => (
    <div style={blockStyle}>
        <strong>CodeBlock:</strong> <code>{args.node.c[1]}</code>
    </div>
);

const BulletList = (args: NodeArgs<BulletListBlock>) => (
    <div style={blockStyle}>
        <strong>BulletList:</strong>
        {renderChildren(args)}
    </div>
);

const OrderedList = (args: NodeArgs<OrderedListBlock>) => (
    <div style={blockStyle}>
        <strong>OrderedList(start={args.node.c[0][0]}):</strong>
        {renderChildren(args)}
    </div>
);

const BlockQuote = (args: NodeArgs<BlockQuoteBlock>) => (
    <div style={blockStyle}>
        <strong>BlockQuote:</strong>
        {renderChildren(args)}
    </div>
);

const Div = (args: NodeArgs<DivBlock>) => (
    <div style={blockStyle}>
        <strong>Div:</strong>
        {renderChildren(args)}
    </div>
);

const HorizontalRule = (_args: NodeArgs<HorizontalRuleBlock>) => (
    <div style={blockStyle}>
        <strong>HorizontalRule</strong>
    </div>
);

const RawBlock = (args: NodeArgs<RawBlock>) => (
    <div style={blockStyle}>
        <strong>RawBlock({args.node.c[0]}):</strong> {args.node.c[1]}
    </div>
);

const Figure = (args: NodeArgs<FigureBlock>) => (
    <div style={blockStyle}>
        <strong>Figure:</strong>
        {renderChildren(args)}
    </div>
);

// Temporary block components registry (will be merged into UnifiedRegistry below)
const BlockComponents: Record<string, (props: any) => React.ReactNode> = {
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

export const Block = (args: NodeArgs<BlockNode>) => renderNode(args, args.node.t)

const Str = (args: NodeArgs<StrInline>) => (
    <span style={inlineStyle}><strong>Str:</strong> {args.node.c}</span>
);

const Space = (_args: NodeArgs<SpaceInline>) => (
    <span style={inlineStyle}><strong>Space</strong></span>
);

const SoftBreak = (_args: NodeArgs<SoftBreakInline>) => (
    <span style={inlineStyle}><strong>SoftBreak</strong></span>
);

const LineBreak = (_args: NodeArgs<LineBreakInline>) => (
    <span style={inlineStyle}><strong>LineBreak</strong></span>
);

const Emph = (args: NodeArgs<EmphInline>) => (
    <span style={inlineStyle}>
        <strong>Emph:</strong> {renderChildren(args)}
    </span>
);

const Strong = (args: NodeArgs<StrongInline>) => (
    <span style={inlineStyle}>
        <strong>Strong:</strong> {renderChildren(args)}
    </span>
);

const Code = (args: NodeArgs<CodeInline>) => (
    <span style={inlineStyle}><strong>Code:</strong> {args.node.c[1]}</span>
);

const Link = (args: NodeArgs<LinkInline>) => (
    <span style={inlineStyle}>
        <strong>Link({args.node.c[2][0]}):</strong> {renderChildren(args)}
    </span>
);

const Image = (args: NodeArgs<ImageInline>) => (
    <span style={inlineStyle}>
        <strong>Image({args.node.c[2][0]}):</strong> {renderChildren(args)}
    </span>
);

const Span = (args: NodeArgs<SpanInline>) => (
    <span style={inlineStyle}>
        <strong>Span:</strong> {renderChildren(args)}
    </span>
);

// Temporary inline components registry (will be merged into UnifiedRegistry below)
const InlineComponents: Record<string, (props: any) => React.ReactNode> = {
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
};

const Inline = (args: NodeArgs<InlineNode>) => renderNode(args, args.node.t)

/**
 * Unified Registry combining all Block and Inline components, plus Block and Inline wrappers
 */
export const componentRegistry: Record<string, (props: any) => React.ReactNode> = {
    ...BlockComponents,
    ...InlineComponents,
    Block,
    Inline,
};

/**
 * Unified Node component that delegates to Block or Inline based on type
 */
const Node = ({
    node,
    onNavigateToDocument,
    setLocalAst
}: {
    node: BlockNode | InlineNode,
    onNavigateToDocument?: (path: string, anchor: string | null) => void,
    setLocalAst: (newNode: BlockNode | InlineNode) => void
}) => {
    const registries = useContext(RegistryContext);
    const registry = registries?.registry ?? componentRegistry;

    // Check if it's a Block type by looking at common block tags
    const blockTypes = ['Para', 'Plain', 'Header', 'CodeBlock', 'BulletList', 'OrderedList', 'BlockQuote', 'Div', 'HorizontalRule', 'RawBlock', 'Figure'];
    const isBlock = blockTypes.includes(node.t);

    if (isBlock) {
        const BlockComponent = registry['Block'];
        if (!BlockComponent) {
            return <div style={blockStyle}><strong>Block wrapper not registered</strong></div>;
        }
        return <BlockComponent
            node={node as BlockNode}
            onNavigateToDocument={onNavigateToDocument}
            setLocalAst={setLocalAst as (newBlock: BlockNode) => void}
        />;
    } else {
        const InlineComponent = registry['Inline'];
        if (!InlineComponent) {
            return <span style={inlineStyle}><strong>Inline wrapper not registered</strong></span>;
        }
        return <InlineComponent
            node={node as InlineNode}
            onNavigateToDocument={onNavigateToDocument}
            setLocalAst={setLocalAst as (newInline: InlineNode) => void}
        />;
    }
};

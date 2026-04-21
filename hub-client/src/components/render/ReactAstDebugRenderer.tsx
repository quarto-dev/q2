import React, { createContext, useContext, useRef, useState, useCallback, useMemo } from 'react';
import type { NodeAttribution } from '../../services/attribution';
import type { SerializableSourceInfo, RustFileInfo } from '@quarto/pandoc-types';
import { SourceInfoReconstructor } from '@quarto/annotated-qmd';
import type { SourceContext } from '@quarto/pandoc-types';
import { AttributionContext } from '../../hooks/useAttribution';
import { getNodeAttribution } from '../../services/attribution';

// Context for unified component registry
const RegistryContext = createContext<{
    registry: Record<string, (props: any) => React.ReactNode>;
} | null>(null);

// Context for per-node attribution queries
export const NodeAttributionContext = createContext<{
    getNodeAttribution: (sourceInfoId: number) => NodeAttribution | null;
} | null>(null);

/**
 * Simplified Pandoc AST types for rendering
 */
export interface PandocAST {
    'pandoc-api-version': [number, number, number];
    meta: Record<string, unknown>;
    blocks: BlockNode[];
    astContext?: {
        sourceInfoPool: SerializableSourceInfo[];
        files: RustFileInfo[];
    };
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
    /** Current file path for resolving relative image paths */
    currentFilePath: string;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
    /** Optional controlled current slide index. If provided, component uses this instead of internal state. */
    currentSlide?: number;
    /** Callback when current slide changes (for controlled mode). */
    onSlideChange?: (slideIndex: number) => void;
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
export function Ast({ astJson, currentFilePath: _currentFilePath, onNavigateToDocument, setAst, currentSlide: _currentSlide, onSlideChange: _onSlideChange, registry = componentRegistry }: PandocAstRendererProps) {
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

    const AstComponent = registry['Ast'];

    return (
        <RegistryContext.Provider value={{ registry }}>
            <AstComponent ast={ast} onNavigateToDocument={onNavigateToDocument} setAst={setAst} />
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
    // Ast type
    Ast: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as PandocAST).blocks.map((block, i) => (
            <Node key={i} node={block} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newBlock: BlockNode | InlineNode) => {
                    const newBlocks = [...(node as PandocAST).blocks];
                    newBlocks[i] = newBlock as BlockNode;
                    setLocalAst({ ...(node as PandocAST), blocks: newBlocks });
                }}
            />
        )),
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
            <li key={i}>{item.map((block, j) => (
                <Node key={JSON.stringify([i, j])} node={block} onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={(newBlock: BlockNode | InlineNode) => {
                        const newItems = [...(node as BulletListBlock).c];
                        const newItem = [...newItems[i]];
                        newItem[j] = newBlock as BlockNode;
                        newItems[i] = newItem;
                        setLocalAst({ t: 'BulletList', c: newItems });
                    }}
                />
            ))}</li>
        )),
    OrderedList: ({ node, setLocalAst, onNavigateToDocument }) =>
        (node as OrderedListBlock).c[1].map((item, i) => (
            <li key={i}>{item.map((block, j) => (
                <Node key={JSON.stringify([i, j])} node={block} onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={(newBlock: BlockNode | InlineNode) => {
                        const newItems = [...(node as OrderedListBlock).c[1]];
                        const newItem = [...newItems[i]];
                        newItem[j] = newBlock as BlockNode;
                        newItems[i] = newItem;
                        setLocalAst({ t: 'OrderedList', c: [(node as OrderedListBlock).c[0], newItems] });
                    }}
                />
            ))}</li>
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
    // Check if this is a PandocAST object (has blocks and pandoc-api-version)
    const nodeType = (node as any).t || ((node as any).blocks && (node as any)['pandoc-api-version'] ? 'Ast' : undefined);

    const renderer = renderChildrenRegistry[nodeType];
    if (!renderer) {
        console.warn(`No renderer found for node type: ${nodeType}`);
        return null;
    }

    return renderer({ node, setLocalAst, onNavigateToDocument });
}

export const renderNode = (args: NodeArgs<BlockNode | InlineNode>, type: string) => {
    const registries = useContext(RegistryContext);
    const registry = registries?.registry ?? componentRegistry;

    // Check if it's a Block type by looking at common block tags
    const blockTypes = ['Para', 'Plain', 'Header', 'CodeBlock', 'BulletList', 'OrderedList', 'BlockQuote', 'Div', 'HorizontalRule', 'RawBlock', 'Figure'];
    const isBlock = blockTypes.includes(type);

    // First try to use Block/Inline wrapper (matches Node component behavior)
    if (isBlock) {
        const BlockComponent = registry['Block'];
        if (BlockComponent) {
            return <BlockComponent {...args} />;
        }
    } else {
        const InlineComponent = registry['Inline'];
        if (InlineComponent) {
            return <InlineComponent {...args} />;
        }
    }

    // Fall back to direct component lookup
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

export const Block = (args: NodeArgs<BlockNode>) => {
    const registries = useContext(RegistryContext);
    const registry = registries?.registry ?? componentRegistry;

    const Component = registry[args.node.t];
    return Component ? <Component {...args} /> : <div style={blockStyle}><strong>Not registered: {args.node.t}</strong></div>;
}

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

const Inline = (args: NodeArgs<InlineNode>) => {
    const registries = useContext(RegistryContext);
    const registry = registries?.registry ?? componentRegistry;

    const Component = registry[args.node.t];
    return Component ? <Component {...args} /> : <span style={inlineStyle}><strong>Not registered: {args.node.t}</strong></span>;
}

const AstRenderer = ({ ast, onNavigateToDocument, setAst }: {
    ast: PandocAST;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
}) => {
    // Extract attribution context and build getNodeAttribution closure
    const attributionCtx = useContext(AttributionContext);
    const astContext = ast.astContext;

    const nodeAttributionValue = useMemo(() => {
        if (!astContext || !attributionCtx) return null;

        try {
            // Populate files[0].content from the Automerge source text
            const sourceContext: SourceContext = {
                files: astContext.files.map((f, idx) => ({
                    id: idx,
                    path: f.name,
                    content: idx === 0 ? attributionCtx.sourceText : (f.content ?? ''),
                })),
            };

            const reconstructor = new SourceInfoReconstructor(
                astContext.sourceInfoPool,
                sourceContext,
            );

            // Cache node attribution results — invalidated automatically when
            // this useMemo recomputes (new astContext or attributionCtx)
            const cache = new Map<number, NodeAttribution | null>();

            return {
                getNodeAttribution: (sourceInfoId: number) => {
                    const cached = cache.get(sourceInfoId);
                    if (cached !== undefined) return cached;
                    const result = getNodeAttribution(
                        sourceInfoId,
                        reconstructor,
                        attributionCtx.source,
                        attributionCtx.identities,
                    );
                    cache.set(sourceInfoId, result);
                    return result;
                },
            };
        } catch {
            return null;
        }
    }, [astContext, attributionCtx]);

    // Use internally-computed value, falling back to any externally-provided context
    const externalNodeAttr = useContext(NodeAttributionContext);
    const effectiveNodeAttr = nodeAttributionValue ?? externalNodeAttr;

    // Ref to avoid stale closures in event handlers
    const effectiveNodeAttrRef = useRef(effectiveNodeAttr);
    effectiveNodeAttrRef.current = effectiveNodeAttr;

    // Hover state for the single floating attribution badge
    const [hoveredAttr, setHoveredAttr] = useState<{
        attr: NodeAttribution;
        rect: DOMRect;
    } | null>(null);

    // Event-delegated hover: one handler on the container instead of N on each node
    const handleMouseOver = useCallback((e: React.MouseEvent) => {
        const ctx = effectiveNodeAttrRef.current;
        if (!ctx) return;
        const target = e.target as HTMLElement;
        const wrap = target.closest('.q2-attr-wrap[data-sid]') as HTMLElement | null;
        if (!wrap) {
            setHoveredAttr(null);
            return;
        }
        const sid = Number(wrap.getAttribute('data-sid'));
        if (Number.isNaN(sid)) return;
        const attr = ctx.getNodeAttribution(sid);
        if (attr) {
            setHoveredAttr({ attr, rect: wrap.getBoundingClientRect() });
        }
    }, []);

    const handleMouseOut = useCallback((e: React.MouseEvent) => {
        const related = e.relatedTarget as HTMLElement | null;
        if (!related?.closest?.('.q2-attr-wrap[data-sid]')) {
            setHoveredAttr(null);
        }
    }, []);

    const tree = (
        <div
            className="pandoc-content-debug"
            style={{ padding: '20px', fontSize: '16px' }}
            onMouseOver={effectiveNodeAttr ? handleMouseOver : undefined}
            onMouseOut={effectiveNodeAttr ? handleMouseOut : undefined}
        >
            {renderChildren({
                node: ast as any,
                setLocalAst: setAst as any,
                onNavigateToDocument
            })}
            {hoveredAttr && (
                <AttributionBadge
                    attr={hoveredAttr.attr}
                    style={{
                        position: 'fixed',
                        top: hoveredAttr.rect.bottom + 2,
                        left: hoveredAttr.rect.left,
                    }}
                />
            )}
        </div>
    );

    // Only wrap with NodeAttributionContext if we built one internally.
    // Otherwise, allow any external provider to pass through.
    return nodeAttributionValue ? (
        <NodeAttributionContext.Provider value={nodeAttributionValue}>
            <style>{attributionStyles}</style>
            {tree}
        </NodeAttributionContext.Provider>
    ) : (
        effectiveNodeAttr ? <><style>{attributionStyles}</style>{tree}</> : tree
    );
};

/**
 * Unified Registry combining all Block and Inline components, plus Block and Inline wrappers
 */
export const componentRegistry: Record<string, (props: any) => React.ReactNode> = {
    ...BlockComponents,
    ...InlineComponents,
    Block,
    Inline,
    Ast: AstRenderer,
};

/**
 * Unified Node component that delegates to Block or Inline based on type
 */
/** Format a timestamp as a relative time string */
function formatRelativeTime(timestamp: number): string {
    const now = Date.now();
    // Automerge timestamps may be in seconds — normalize to ms
    const tsMs = timestamp < 1e12 ? timestamp * 1000 : timestamp;
    const diffMs = now - tsMs;
    const diffSec = Math.floor(diffMs / 1000);
    if (diffSec < 60) return 'just now';
    const diffMin = Math.floor(diffSec / 60);
    if (diffMin < 60) return `${diffMin}m ago`;
    const diffHr = Math.floor(diffMin / 60);
    if (diffHr < 24) return `${diffHr}h ago`;
    const diffDay = Math.floor(diffHr / 24);
    return `${diffDay}d ago`;
}

/** Styled tooltip that appears on hover, colored to match the author */
function AttributionBadge({ attr, style }: {
    attr: { name: string; time: number; color: string };
    style?: React.CSSProperties;
}) {
    return <span className="q2-attr-badge" style={{
        '--attr-color': attr.color,
        ...style,
    } as React.CSSProperties}>
        <span className="q2-attr-badge-dot" style={{ backgroundColor: attr.color }} />
        {attr.name} <span className="q2-attr-badge-time">{formatRelativeTime(attr.time)}</span>
    </span>;
}

/** Hover styles for attribution badges — rendered once per AstRenderer */
const attributionStyles = `
    .q2-attr-wrap { position: relative; }
    .q2-attr-badge {
        display: inline-block;
        z-index: 10;
        font-size: 10px;
        line-height: 1;
        white-space: nowrap;
        padding: 2px 6px;
        border-radius: 3px;
        background: #fff;
        border: 1px solid var(--attr-color);
        color: var(--attr-color);
        font-weight: 600;
        pointer-events: none;
    }
    .q2-attr-badge-dot {
        display: inline-block;
        width: 6px;
        height: 6px;
        border-radius: 50%;
        margin-right: 3px;
        vertical-align: middle;
    }
    .q2-attr-badge-time {
        font-weight: 400;
        opacity: 0.7;
    }
`;

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
    const attributionCtx = useContext(NodeAttributionContext);

    // Resolve attribution for this node
    const sourceInfoId = (node as { s?: number }).s;
    const attr = (sourceInfoId != null && attributionCtx)
        ? attributionCtx.getNodeAttribution(sourceInfoId)
        : null;

    // Check if it's a Block type by looking at common block tags
    const blockTypes = ['Para', 'Plain', 'Header', 'CodeBlock', 'BulletList', 'OrderedList', 'BlockQuote', 'Div', 'HorizontalRule', 'RawBlock', 'Figure'];
    const isBlock = blockTypes.includes(node.t);

    if (isBlock) {
        const BlockComponent = registry['Block'];
        if (!BlockComponent) {
            return <div style={blockStyle}><strong>Block wrapper not registered</strong></div>;
        }
        return <div
            className={attr ? 'q2-attr-wrap' : undefined}
            data-sid={attr ? sourceInfoId : undefined}
            style={attr ? { color: attr.color } : undefined}
        >
            <BlockComponent
                node={node as BlockNode}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={setLocalAst as (newBlock: BlockNode) => void}
            />
        </div>;
    } else {
        const InlineComponent = registry['Inline'];
        if (!InlineComponent) {
            return <span style={inlineStyle}><strong>Inline wrapper not registered</strong></span>;
        }
        return <span
            className={attr ? 'q2-attr-wrap' : undefined}
            data-sid={attr ? sourceInfoId : undefined}
            style={attr ? { color: attr.color } : undefined}
        >
            <InlineComponent
                node={node as InlineNode}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={setLocalAst as (newInline: InlineNode) => void}
            />
        </span>;
    }
};

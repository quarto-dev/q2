import React from 'react';
import { Node, renderChildren, useAttributionHover } from '@quarto/preview-renderer/framework';
import type {
    InlineNode,
    NodeArgs,
    PandocAST,
    ParaBlock,
    PlainBlock,
    HeaderBlock,
    CodeBlock as CodeBlockType,
    BulletListBlock,
    OrderedListBlock,
    BlockQuoteBlock,
    DivBlock,
    HorizontalRuleBlock,
    RawBlock as RawBlockType,
    FigureBlock,
    StrInline,
    SpaceInline,
    SoftBreakInline,
    LineBreakInline,
    EmphInline,
    StrongInline,
    CodeInline,
    LinkInline,
    ImageInline,
    SpanInline,
    QuotedInline,
} from '@quarto/preview-renderer/framework';
import { blockStyle, inlineStyle } from './styles';
import { dataOffProps, DATA_STR_TEXT } from './sourceOffset';

export const Para = (args: NodeArgs<ParaBlock>) => (
    <div style={blockStyle} {...dataOffProps(args.node)}>
        <strong>Para:</strong> {renderChildren(args)}
    </div>
);

export const Plain = (args: NodeArgs<PlainBlock>) => (
    <div style={blockStyle} {...dataOffProps(args.node)}>
        <strong>Plain:</strong> {renderChildren(args)}
    </div>
);

export const Header = (args: NodeArgs<HeaderBlock>) => (
    <div style={blockStyle} {...dataOffProps(args.node)}>
        <strong>Header(level={args.node.c[0]}):</strong> {renderChildren(args)}
    </div>
);

export const CodeBlock = (args: NodeArgs<CodeBlockType>) => (
    <div style={blockStyle} {...dataOffProps(args.node)}>
        <strong>CodeBlock:</strong> <code>{args.node.c[1]}</code>
    </div>
);

export const BulletList = (args: NodeArgs<BulletListBlock>) => (
    <div style={blockStyle} {...dataOffProps(args.node)}>
        <strong>BulletList:</strong>
        {renderChildren(args)}
    </div>
);

export const OrderedList = (args: NodeArgs<OrderedListBlock>) => (
    <div style={blockStyle} {...dataOffProps(args.node)}>
        <strong>OrderedList(start={args.node.c[0][0]}):</strong>
        {renderChildren(args)}
    </div>
);

export const BlockQuote = (args: NodeArgs<BlockQuoteBlock>) => (
    <div style={blockStyle} {...dataOffProps(args.node)}>
        <strong>BlockQuote:</strong>
        {renderChildren(args)}
    </div>
);

export const Div = (args: NodeArgs<DivBlock>) => (
    <div style={blockStyle} {...dataOffProps(args.node)}>
        <strong>Div:</strong>
        {renderChildren(args)}
    </div>
);

export const HorizontalRule = (args: NodeArgs<HorizontalRuleBlock>) => (
    <div style={blockStyle} {...dataOffProps(args.node)}>
        <strong>HorizontalRule</strong>
    </div>
);

export const RawBlock = (args: NodeArgs<RawBlockType>) => (
    <div style={blockStyle} {...dataOffProps(args.node)}>
        <strong>RawBlock({args.node.c[0]}):</strong> {args.node.c[1]}
    </div>
);

// Body via renderChildren (framework's per-Pandoc-tag walker); the bordered
// "Caption: ShortCaption" branch lives here so q2-debug preserves its
// historical visible output. The framework's `renderChildrenRegistry.Figure`
// renders only the body blocks (consistent with every other entry).
export const Figure = (args: NodeArgs<FigureBlock>) => (
    <div style={blockStyle} {...dataOffProps(args.node)}>
        <strong>Figure:</strong>
        {renderChildren(args)}
        {args.node.c[1][0] && (
            <div><em>Caption:</em> {args.node.c[1][0]!.map((inline, i) => (
                <Node key={i} node={inline} onNavigateToDocument={args.onNavigateToDocument}
                    setLocalAst={(newInline) => {
                        const newCaption = [...args.node.c[1][0]!];
                        newCaption[i] = newInline as InlineNode;
                        args.setLocalAst({ t: 'Figure', c: [args.node.c[0], [newCaption, args.node.c[1][1]], args.node.c[2]] });
                    }}
                />
            ))}</div>
        )}
    </div>
);

export const BlockComponents: Record<string, (props: any) => React.ReactNode> = {
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

export const Str = (args: NodeArgs<StrInline>) => (
    <span style={inlineStyle} {...dataOffProps(args.node)}>
        <strong>Str:</strong> <span {...{ [DATA_STR_TEXT]: '' }}>{args.node.c}</span>
    </span>
);

export const Space = (args: NodeArgs<SpaceInline>) => (
    <span style={inlineStyle} {...dataOffProps(args.node)}><strong>Space</strong></span>
);

export const SoftBreak = (args: NodeArgs<SoftBreakInline>) => (
    <span style={inlineStyle} {...dataOffProps(args.node)}><strong>SoftBreak</strong></span>
);

export const LineBreak = (args: NodeArgs<LineBreakInline>) => (
    <span style={inlineStyle} {...dataOffProps(args.node)}><strong>LineBreak</strong></span>
);

export const Emph = (args: NodeArgs<EmphInline>) => (
    <span style={inlineStyle} {...dataOffProps(args.node)}>
        <strong>Emph:</strong> {renderChildren(args)}
    </span>
);

export const Strong = (args: NodeArgs<StrongInline>) => (
    <span style={inlineStyle} {...dataOffProps(args.node)}>
        <strong>Strong:</strong> {renderChildren(args)}
    </span>
);

export const Code = (args: NodeArgs<CodeInline>) => (
    <span style={inlineStyle} {...dataOffProps(args.node)}><strong>Code:</strong> {args.node.c[1]}</span>
);

export const Link = (args: NodeArgs<LinkInline>) => (
    <span style={inlineStyle} {...dataOffProps(args.node)}>
        <strong>Link({args.node.c[2][0]}):</strong> {renderChildren(args)}
    </span>
);

export const Image = (args: NodeArgs<ImageInline>) => (
    <span style={inlineStyle} {...dataOffProps(args.node)}>
        <strong>Image({args.node.c[2][0]}):</strong> {renderChildren(args)}
    </span>
);

export const Span = (args: NodeArgs<SpanInline>) => (
    <span style={inlineStyle} {...dataOffProps(args.node)}>
        <strong>Span:</strong> {renderChildren(args)}
    </span>
);

export const Quoted = (args: NodeArgs<QuotedInline>) => (
    <span style={inlineStyle} {...dataOffProps(args.node)}>
        <strong>Quoted({args.node.c[0].t}):</strong> {renderChildren(args)}
    </span>
);

export const InlineComponents: Record<string, (props: any) => React.ReactNode> = {
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
    Quoted,
};

/**
 * q2-debug document root. Delegates the badge stylesheet / hover
 * handler / overlay wiring to `useAttributionHover`. Off-path the
 * hook returns inert `hostProps` / `null` overlay+stylesheet, so the
 * rendered DOM is byte-identical to pre-attribution.
 */
export const AstRenderer = ({ ast, onNavigateToDocument, setAst }: {
    ast: PandocAST;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
}) => {
    const attr = useAttributionHover();
    return (
        <>
            {attr.stylesheet}
            <div
                className="pandoc-content-debug"
                style={{ padding: '20px', fontSize: '16px' }}
                {...attr.hostProps}
            >
                {renderChildren({
                    node: ast as any,
                    setLocalAst: setAst as any,
                    onNavigateToDocument,
                })}
                {attr.overlay}
            </div>
        </>
    );
};

import React from 'react';
import { Node, renderChildren } from '../framework/dispatch';
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
} from '../framework/types';
import { blockStyle, inlineStyle } from './styles';

export const Para = (args: NodeArgs<ParaBlock>) => (
    <div style={blockStyle}>
        <strong>Para:</strong> {renderChildren(args)}
    </div>
);

export const Plain = (args: NodeArgs<PlainBlock>) => (
    <div style={blockStyle}>
        <strong>Plain:</strong> {renderChildren(args)}
    </div>
);

export const Header = (args: NodeArgs<HeaderBlock>) => (
    <div style={blockStyle}>
        <strong>Header(level={args.node.c[0]}):</strong> {renderChildren(args)}
    </div>
);

export const CodeBlock = (args: NodeArgs<CodeBlockType>) => (
    <div style={blockStyle}>
        <strong>CodeBlock:</strong> <code>{args.node.c[1]}</code>
    </div>
);

export const BulletList = (args: NodeArgs<BulletListBlock>) => (
    <div style={blockStyle}>
        <strong>BulletList:</strong>
        {renderChildren(args)}
    </div>
);

export const OrderedList = (args: NodeArgs<OrderedListBlock>) => (
    <div style={blockStyle}>
        <strong>OrderedList(start={args.node.c[0][0]}):</strong>
        {renderChildren(args)}
    </div>
);

export const BlockQuote = (args: NodeArgs<BlockQuoteBlock>) => (
    <div style={blockStyle}>
        <strong>BlockQuote:</strong>
        {renderChildren(args)}
    </div>
);

export const Div = (args: NodeArgs<DivBlock>) => (
    <div style={blockStyle}>
        <strong>Div:</strong>
        {renderChildren(args)}
    </div>
);

export const HorizontalRule = (_args: NodeArgs<HorizontalRuleBlock>) => (
    <div style={blockStyle}>
        <strong>HorizontalRule</strong>
    </div>
);

export const RawBlock = (args: NodeArgs<RawBlockType>) => (
    <div style={blockStyle}>
        <strong>RawBlock({args.node.c[0]}):</strong> {args.node.c[1]}
    </div>
);

// Body via renderChildren (framework's per-Pandoc-tag walker); the bordered
// "Caption: ShortCaption" branch lives here so q2-debug preserves its
// historical visible output. The framework's `renderChildrenRegistry.Figure`
// renders only the body blocks (consistent with every other entry).
export const Figure = (args: NodeArgs<FigureBlock>) => (
    <div style={blockStyle}>
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
    <span style={inlineStyle}><strong>Str:</strong> {args.node.c}</span>
);

export const Space = (_args: NodeArgs<SpaceInline>) => (
    <span style={inlineStyle}><strong>Space</strong></span>
);

export const SoftBreak = (_args: NodeArgs<SoftBreakInline>) => (
    <span style={inlineStyle}><strong>SoftBreak</strong></span>
);

export const LineBreak = (_args: NodeArgs<LineBreakInline>) => (
    <span style={inlineStyle}><strong>LineBreak</strong></span>
);

export const Emph = (args: NodeArgs<EmphInline>) => (
    <span style={inlineStyle}>
        <strong>Emph:</strong> {renderChildren(args)}
    </span>
);

export const Strong = (args: NodeArgs<StrongInline>) => (
    <span style={inlineStyle}>
        <strong>Strong:</strong> {renderChildren(args)}
    </span>
);

export const Code = (args: NodeArgs<CodeInline>) => (
    <span style={inlineStyle}><strong>Code:</strong> {args.node.c[1]}</span>
);

export const Link = (args: NodeArgs<LinkInline>) => (
    <span style={inlineStyle}>
        <strong>Link({args.node.c[2][0]}):</strong> {renderChildren(args)}
    </span>
);

export const Image = (args: NodeArgs<ImageInline>) => (
    <span style={inlineStyle}>
        <strong>Image({args.node.c[2][0]}):</strong> {renderChildren(args)}
    </span>
);

export const Span = (args: NodeArgs<SpanInline>) => (
    <span style={inlineStyle}>
        <strong>Span:</strong> {renderChildren(args)}
    </span>
);

export const Quoted = (args: NodeArgs<QuotedInline>) => (
    <span style={inlineStyle}>
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

export const AstRenderer = ({ ast, onNavigateToDocument, setAst }: {
    ast: PandocAST;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
}) => (
    <div className="pandoc-content-debug" style={{ padding: '20px', fontSize: '16px' }}>
        {renderChildren({
            node: ast as any,
            setLocalAst: setAst as any,
            onNavigateToDocument
        })}
    </div>
);

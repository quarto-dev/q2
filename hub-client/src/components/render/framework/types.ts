import React from 'react';

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
export type MathInline = { t: 'Math'; c: [{ t: 'DisplayMath' | 'InlineMath' }, string] };
export type QuotedInline = { t: 'Quoted'; c: [{ t: 'SingleQuote' | 'DoubleQuote' }, InlineNode[]] };
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
    | MathInline
    | QuotedInline
    | UnknownInline;

export type NodeArgs<T extends BlockNode | InlineNode> = {
    node: T,
    onNavigateToDocument?: (path: string, anchor: string | null) => void,
    setLocalAst: (newNode: BlockNode | InlineNode) => void
}

/**
 * Format-registry contracts.
 *
 * The framework reserves three registry keys: 'Ast', 'Block', 'Inline'.
 * Each format must register all three. The framework provides no
 * implementations under any of these keys.
 */
export type AstProps = {
    ast: PandocAST;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
};
export type AstComponent = (props: AstProps) => React.ReactNode;
export type DispatcherComponent = (args: NodeArgs<BlockNode | InlineNode>) => React.ReactNode;

export type FormatRegistry = Record<string, (props: any) => React.ReactNode> & {
    Ast: AstComponent;
    Block: DispatcherComponent;
    Inline: DispatcherComponent;
};

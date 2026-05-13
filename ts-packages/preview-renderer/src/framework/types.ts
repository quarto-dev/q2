import React from 'react';

/**
 * Simplified Pandoc AST types for rendering
 */
export interface PandocAST {
    'pandoc-api-version': [number, number, number];
    meta: Record<string, unknown>;
    blocks: BlockNode[];
}

/**
 * Pandoc Attr triple: (id, classes, key/value pairs).
 * Spelled inline in many existing node types; named alias used by
 * the CustomNode shapes below.
 */
export type Attr = [string, string[], [string, string][]];

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
export type LineBlockBlock = { t: 'LineBlock'; c: InlineNode[][] };
export type DefinitionListBlock = { t: 'DefinitionList'; c: [InlineNode[], BlockNode[][]][] };
export type TableBlock = {
    t: 'Table';
    c: [
        Attr,
        [InlineNode[] | null, BlockNode[]],
        [Attr, { t: 'AlignDefault' | 'AlignLeft' | 'AlignRight' | 'AlignCenter' }[], unknown[], unknown[][], unknown[][]],
        [Attr, unknown[]][],
        [Attr, unknown[][]][],
        [Attr, unknown[][]],
    ];
};
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
    | LineBlockBlock
    | DefinitionListBlock
    | TableBlock
    | CustomBlockNode
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
export type UnderlineInline = { t: 'Underline'; c: InlineNode[] };
export type StrikeoutInline = { t: 'Strikeout'; c: InlineNode[] };
export type SuperscriptInline = { t: 'Superscript'; c: InlineNode[] };
export type SubscriptInline = { t: 'Subscript'; c: InlineNode[] };
export type SmallCapsInline = { t: 'SmallCaps'; c: InlineNode[] };
export type RawInlineInline = { t: 'RawInline'; c: [string, string] };
// Pandoc shape: Cite [Citation] [Inline]
// c[0] is the Citations array (Pandoc metadata used for bibliography);
// c[1] is the visible inlines Pandoc fills in for the link text.
// v1 q2-preview renders c[1] only; c[0] is unstructured `unknown[]`
// because the Citation shape is not consumed today.
export type CiteInline = { t: 'Cite'; c: [unknown[], InlineNode[]]; s?: number };
export type NoteInline = { t: 'Note'; c: BlockNode[]; s?: number };
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
    | UnderlineInline
    | StrikeoutInline
    | SuperscriptInline
    | SubscriptInline
    | SmallCapsInline
    | RawInlineInline
    | CiteInline
    | NoteInline
    | CustomInlineNode
    | UnknownInline;

/**
 * Custom node shapes (Plan 2B).
 *
 * After `unwrapCustomNodes` runs in `framework/Ast.tsx`, every
 * `__quarto_custom_node` wire-format Div/Span has been replaced with
 * one of these JS-native shapes. Slot iteration order is preserved
 * across unwrap so `slot_meta` JSON and the slot wrapper sequence
 * match the original wire format on rewrap.
 */
export type Slot =
    | { kind: 'block'; value: BlockNode }
    | { kind: 'inline'; value: InlineNode }
    | { kind: 'blocks'; value: BlockNode[] }
    | { kind: 'inlines'; value: InlineNode[] };

export interface CustomNodeBase {
    type_name: string;
    slots: Record<string, Slot>;
    plain_data: unknown;
    attr: Attr;
    /** Source-info pool index, preserved across unwrap/rewrap. */
    s?: number;
}

export interface CustomBlockNode extends CustomNodeBase { t: 'CustomBlock' }
export interface CustomInlineNode extends CustomNodeBase { t: 'CustomInline' }
export type CustomNode = CustomBlockNode | CustomInlineNode;

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

/**
 * Synthetic registry keys carry component-specific prop shapes
 * distinct from per-tag dispatchers. Typed explicitly so user TSX
 * overrides get compile-time prop-shape checking; Pandoc-tag and
 * CustomNode-`type_name` keys keep their loose `(props: any)` typing
 * via the index signature.
 *
 * - `__fallback__` (Plan 2C) → receives `NodeArgs<CustomBlockNode |
 *   CustomInlineNode>` from the CustomBlock / CustomInline
 *   dispatchers (`dispatchers.tsx:78-82`).
 * - `__title_block__` (Plan 2D) → receives `AstProps`, parallel to
 *   the `Ast` key. The title block operates on document-level state
 *   (`ast.meta`), not on a node in the AST.
 */
export type FormatRegistry = Record<string, (props: any) => React.ReactNode> & {
    Ast: AstComponent;
    Block: DispatcherComponent;
    Inline: DispatcherComponent;
    __fallback__?: (
        args: NodeArgs<CustomBlockNode | CustomInlineNode>,
    ) => React.ReactNode;
    __title_block__?: AstComponent;
};

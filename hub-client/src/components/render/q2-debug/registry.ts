import type { FormatRegistry, NodeArgs, BlockNode, InlineNode } from '@quarto/preview-renderer/framework';
import type { ReactNode } from 'react';
import { Block, Inline } from './dispatchers';
import {
    AstRenderer,
    Para, Plain, Header, CodeBlock, BulletList, OrderedList, BlockQuote, Div, HorizontalRule, RawBlock, Figure,
    LineBlock, DefinitionList, Table,
    Str, Space, SoftBreak, LineBreak, Emph, Strong, Code, Link, Image, Span, Quoted,
    Math, Underline, Strikeout, Superscript, Subscript, SmallCaps, RawInline, Cite, Note,
} from './components';

export { Block, Inline };

type TagComponent = (props: NodeArgs<BlockNode | InlineNode>) => ReactNode;

/** The block-level tags q2-debug registers, each rendered by the inspector node. */
export const BlockComponents: Record<string, TagComponent> = {
    Para, Plain, Header, CodeBlock, BulletList, OrderedList, BlockQuote, Div, HorizontalRule, RawBlock, Figure,
    LineBlock, DefinitionList, Table,
};

/** The inline-level tags q2-debug registers, each rendered by the inspector node. */
export const InlineComponents: Record<string, TagComponent> = {
    Str, Space, SoftBreak, LineBreak, Emph, Strong, Code, Link, Image, Span, Quoted,
    Math, Underline, Strikeout, Superscript, Subscript, SmallCaps, RawInline, Cite, Note,
};

/**
 * q2-debug format registry. The framework reserves the keys 'Ast',
 * 'Block', and 'Inline'; each format must register all three. q2-debug's
 * Block/Inline dispatch to the inspector node (or a user override);
 * 'Ast' is the inspector's document root.
 */
export const q2DebugRegistry: FormatRegistry = {
    ...BlockComponents,
    ...InlineComponents,
    Block,
    Inline,
    Ast: AstRenderer,
};

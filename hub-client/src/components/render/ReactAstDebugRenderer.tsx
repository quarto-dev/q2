/**
 * Phase-1 re-export shim (plan-2pre).
 *
 * The renderer was carved up into `framework/` (format-agnostic dispatch +
 * types + the typed registry contract) and `q2-debug/` (bordered-box leaves
 * + dispatchers + format registry). This shim re-exports everything under
 * its old names so existing consumers keep compiling while Phase 2 migrates
 * each consumer to the new locations one commit at a time. Deleted at the
 * end of Phase 2 (step 2.14).
 */

export { Ast, renderChildren, renderNode, Node } from './framework';
export type {
    PandocAST,
    ParaBlock,
    PlainBlock,
    HeaderBlock,
    CodeBlock,
    BulletListBlock,
    OrderedListBlock,
    BlockQuoteBlock,
    DivBlock,
    HorizontalRuleBlock,
    RawBlock,
    FigureBlock,
    UnknownBlock,
    BlockNode,
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
    MathInline,
    QuotedInline,
    UnknownInline,
    InlineNode,
    NodeArgs,
} from './framework';
export { Block, Inline, blockStyle, inlineStyle } from './q2-debug';
export { q2DebugRegistry as componentRegistry } from './q2-debug';

import type { FormatRegistry } from '../framework';
import * as Blocks from './blocks';
import * as Inlines from './inlines';
import * as Custom from './custom';
import { Inline, CustomBlock, CustomInline } from './dispatchers';
import { CommentBlock } from './custom/CommentBlock';
import { MermaidCodeBlock } from './blocks/MermaidCodeBlock';
import { PreviewDocument } from './PreviewDocument';

/**
 * q2-preview format registry. The framework reserves the keys 'Ast',
 * 'Block', and 'Inline'; each format must register all three.
 *
 * 2B populates the registry with every Pandoc base-type leaf
 * (14 blocks + 20 inlines). 2C extends with the type-keyed CustomNode
 * components (`Callout`, `Theorem`, `Proof`, `FloatRefTarget`,
 * `Equation`, `CrossrefResolvedRef`) plus a `__fallback__` entry, and
 * the `CustomBlock` / `CustomInline` dispatchers that look them up
 * by `node.type_name`.
 *
 * Plan 2D adds the synthetic `__title_block__` entry (parallel to
 * `__fallback__`) for the document-title-block chrome. Synthetic
 * keys (`__fallback__`, `__title_block__`) carry component-specific
 * prop shapes (NodeArgs vs AstProps), distinct from the per-tag
 * `Block`/`Inline` dispatcher contract — see `framework/types.ts`'s
 * `FormatRegistry` typed optional entries.
 *
 * Pandoc tag names and CustomNode `type_name`s are namespace-disjoint
 * by project policy (locked at build time by the namespace-disjoint
 * test in `registry.test.ts`). One merged map carries both — a user
 * TSX override of either kind layers on via the same merge site in
 * `entry.tsx`'s PreviewRoot.
 *
 * `mergedPreviewRegistry` (in entry.tsx's PreviewRoot) layers user-TSX
 * exports on top via `{ ...previewRegistry, ...customRegistry }`, so
 * a user override of `Para` / `Image` / `Callout` / `__title_block__`
 * / etc. wins over the built-in.
 */
export const previewRegistry: FormatRegistry = {
    ...Blocks,
    ...Inlines,
    ...Custom, // Callout, Theorem, Proof, FloatRefTarget, Equation, CrossrefResolvedRef, Fallback, PreviewTitleBlock
    // bd-5m4ga0s1: mermaid-aware CodeBlock wrapper — renders
    // ```mermaid blocks as diagrams (CDN mermaid.js), delegates
    // everything else to Blocks.CodeBlock. Overrides the plain entry
    // from the `...Blocks` spread; user render-components overrides
    // of `CodeBlock` still layer on top via mergedPreviewRegistry.
    CodeBlock: MermaidCodeBlock,
    __fallback__: Custom.Fallback,
    __title_block__: Custom.PreviewTitleBlock,
    // Default comment/reaction chrome (custom/CommentBlock.tsx): wraps
    // the dispatchers.tsx Block dispatcher, extracting
    // `quarto-edit-comment` spans into corner chrome. A user
    // render-components override of `Block` still wins via
    // mergedPreviewRegistry.
    Block: CommentBlock,
    Inline,
    CustomBlock,
    CustomInline,
    Ast: PreviewDocument,
};

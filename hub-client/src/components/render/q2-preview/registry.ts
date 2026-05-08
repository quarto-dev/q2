import type { FormatRegistry } from '../framework';
import { Block, Inline } from './dispatchers';
import { PreviewDocument } from './PreviewDocument';

/**
 * q2-preview format registry. The framework reserves the keys 'Ast',
 * 'Block', and 'Inline'; each format must register all three.
 * q2-preview's `Block`/`Inline` are the muted-gray placeholder
 * dispatchers; `Ast` is the unstyled document-root wrapper.
 *
 * Plan 2B fills the rest with real-HTML leaves.
 */
export const previewRegistry: FormatRegistry = {
    Block,
    Inline,
    Ast: PreviewDocument,
};

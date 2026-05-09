import type { FormatRegistry } from '../framework';
import * as Blocks from './blocks';
import * as Inlines from './inlines';
import { Block, Inline } from './dispatchers';
import { PreviewDocument } from './PreviewDocument';

/**
 * q2-preview format registry. The framework reserves the keys 'Ast',
 * 'Block', and 'Inline'; each format must register all three.
 *
 * 2B populates the registry with every Pandoc base-type leaf
 * (14 blocks + 20 inlines). CustomBlock / CustomInline keys are
 * deliberately not registered — Plan 2C ships the per-`type_name`
 * components and the dispatcher entries that look them up; until
 * 2C lands, custom-node wrappers fall through to dispatchers.tsx's
 * muted-gray "(not yet implemented)" placeholder.
 *
 * `mergedRegistry` (in entry.tsx's PreviewRoot) layers user-TSX
 * exports on top via `{ ...previewRegistry, ...customRegistry }`,
 * so a user override of `Para` / `Image` / etc. wins over the
 * built-in.
 */
export const previewRegistry: FormatRegistry = {
    ...Blocks,
    ...Inlines,
    Block,
    Inline,
    Ast: PreviewDocument,
};

import { createContext } from 'react';
import type { BlockNode } from '../framework/types';
import type { ReachabilityClass, SourceIndexEntry, ResolvedSource } from './sourceIndex';

/**
 * q2-preview-specific context. Carries values that don't belong on the
 * framework's `RegistryContext` because q2-debug doesn't need them.
 *
 * - `currentFilePath`: resolves relative image paths and qmd link targets.
 * - `pool`: the source-info pool from the last rendered AST JSON.
 * - `commitTextEdit`: commit a raw-QMD text edit via the text channel (Plan 2b).
 * - `commitSubtreeEdit`: commit a pre-parsed Pandoc subtree edit via the
 *   subtree channel (Plan 2b). Used by render-component authors.
 * - `content`: QMD source content that produced the current render.
 * - `editTarget`: block currently being edited (poolId + measured DOMRect),
 *   or null when no block is active (Plan 2b: rect needed for P1 no-reflow sizing).
 * - `setEditTarget`: activate a block for editing; null clears.
 * - `sourceIndex`: SourceInfo-value index from the untransformed AST (Plan 2a).
 * - `resolveSource`: look up a transformed block's source counterpart (Plan 2a).
 *
 * The default value is `null` — leaves should treat absence as a bug.
 */

export type { ReachabilityClass, SourceIndexEntry, ResolvedSource };

export interface PreviewContextValue {
    currentFilePath: string;
    /** Source-info pool from the rendered AST — `astContext.p` array. */
    pool?: unknown[];
    /** Commit a raw-QMD text edit (text channel, Plan 2b). */
    commitTextEdit?: (destinationSourceInfoJson: string, newText: string) => void;
    /** Commit a Pandoc subtree replacement (subtree channel, Plan 2b). Used by render-component authors via `usePreviewEdit`. */
    commitSubtreeEdit?: (destinationSourceInfoJson: string, modifiedBlock: BlockNode) => void;
    /** The QMD source content that produced the current render. Used by editable blocks to slice source bytes. */
    content?: string;
    /** The block currently being edited (poolId + measured DOMRect for P1 no-reflow sizing), or null. */
    editTarget?: { poolId: string | number; rect: DOMRect } | null;
    /** Activate a block for editing (pass its poolId + measured DOMRect), or pass null to clear. */
    setEditTarget?: (target: { poolId: string | number; rect: DOMRect } | null) => void;
    /** SourceInfo-value index from the untransformed AST (Plan 2a). Built once per render. */
    sourceIndex?: Map<string, SourceIndexEntry> | null;
    /** Resolve a transformed block to its source counterpart + reachability class (Plan 2a). */
    resolveSource?: (node: BlockNode) => ResolvedSource | null;
}

export const PreviewContext = createContext<PreviewContextValue | null>(null);

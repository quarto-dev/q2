import { createContext } from 'react';

/**
 * q2-preview-specific context. Carries values that don't belong on the
 * framework's `RegistryContext` because q2-debug doesn't need them.
 *
 * - `currentFilePath`: resolves relative image paths and qmd link targets
 *   in q2-preview leaves (Plan 2B's `Image`, link handlers).
 * - `pool`: the source-info pool from the last rendered AST JSON. Maps
 *   pool-id (string or number key) to the serialised SourceInfo value.
 *   Used by editable block components (Para, Header) to resolve a node's
 *   `s` field to a `destinationSourceInfoJson` for `PreviewNodeEditPayload`.
 * - `commitEdit`: commit a user text edit for `apply_node_edit`.
 *   Provided by `entry.tsx`; calls `setAst` with `PreviewNodeEditPayload`.
 *   `poolId` is the raw value of `block.s`; `newText` is the new QMD text.
 *
 * The default value is `null` — leaves should treat absence as a bug
 * (every q2-preview render is mounted under a `PreviewContext.Provider`
 * by `entry.tsx`'s `PreviewRoot`).
 */
export interface PreviewContextValue {
    currentFilePath: string;
    /** Source-info pool from the rendered AST — `astContext.p` array (Phase 5). */
    pool?: unknown[];
    /** Commit a text edit for a block identified by its pool id (Phase 5). */
    commitEdit?: (poolId: string | number, newText: string) => void;
    /** The QMD source content that produced the current render (rendered-generation snapshot). Used by editable blocks to slice source bytes. */
    content?: string;
    /** Pool id of the block currently being edited, or null when no block is active. */
    editTarget?: string | number | null;
    /** Set the active edit target by pool id, or pass null to clear. */
    setEditTarget?: (id: string | number | null) => void;
}

export const PreviewContext = createContext<PreviewContextValue | null>(null);

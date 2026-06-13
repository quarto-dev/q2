import { createContext } from 'react';
import type React from 'react';
import type { BlockNode } from '../framework/types';
import type { ReachabilityClass, SourceIndexEntry, ResolvedSource } from './sourceIndex';
import type { MutableRefObject } from 'react';

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
 * - `editTarget`: block currently being edited (`anchorR0` / `anchorR1` byte
 *   offsets + `anchorSlice` source text + `contentHeight` + `boxStyle` for the
 *   measure-and-set wrapper), or null when none.
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
    /**
     * The block currently being edited (or null when none is active).
     *
     * - `anchorR0` / `anchorR1`: UTF-8 byte offsets from the pool entry captured
     *   at click time. Used to match the block across re-renders by identity
     *   (not positional ordinal / poolId). When a collaborator inserts/removes a
     *   block above, the ordinal shifts but the byte range stays stable for
     *   existing blocks.
     * - `anchorSlice`: `normalizeLineEndings(sliceBytes(content, anchorR0, anchorR1)).trimEnd()`
     *   captured at activation. The dirty guard compares `normalizeLineEndings(draft).trimEnd()`
     *   against this value; equal → no commit.
     * - `contentHeight` is the content-area height (rect.height minus padding
     *   and border), used as the textarea's height so it fills the content area
     *   exactly even when the element has padding/border (e.g. Bootstrap's
     *   `h2 { padding-bottom: 0.5rem; border-bottom }`).
     * - `boxStyle` is the element's full computed box (margin + padding +
     *   per-side border longhands), captured from `getComputedStyle` at
     *   activation. The measure-and-set edit wrapper replicates it on a
     *   synthetic `<div>` so the textarea's box exactly matches the element it
     *   replaces — preserving vertical spacing AND visible decorations like an
     *   h2's `border-bottom` rule, with zero reflow.
     */
    editTarget?: {
        anchorR0: number;
        anchorR1: number;
        anchorSlice: string;
        contentHeight: number;
        boxStyle: Record<string, string>;
    } | null;
    /** Activate a block for editing, or pass null to clear. */
    setEditTarget?: (target: {
        anchorR0: number;
        anchorR1: number;
        anchorSlice: string;
        contentHeight: number;
        boxStyle: Record<string, string>;
    } | null) => void;
    /**
     * Root-held ref for the in-flight edit draft text. Stable reference across
     * re-renders (no extra state triggers). Seeded with `anchorSlice` when an
     * edit target is activated; reset to null when cleared. The `EditTextarea`
     * component seeds its local useState from this ref on mount.
     *
     * Draft isolation: per-keystroke re-renders are confined to `EditTextarea`
     * only — the ref prevents draft changes from propagating up to PreviewRoot.
     */
    editDraftRef?: MutableRefObject<string | null>;
    /** SourceInfo-value index from the untransformed AST (Plan 2a). Built once per render. */
    sourceIndex?: Map<string, SourceIndexEntry> | null;
    /** Resolve a transformed block to its source counterpart + reachability class (Plan 2a). */
    resolveSource?: (node: BlockNode) => ResolvedSource | null;
    /**
     * Ref pointing to the inner wrapper `<div>` of the currently active
     * measure-and-set edit region (`renderMeasuredEdit`'s inner div).
     *
     * Used by `useBlockEditHover`'s `onPointerUp` to detect whether a mouse
     * click landed inside the open editor and suppress the spurious parent-
     * climb activation that would otherwise occur (Phase 1 bug fix). A click
     * whose target is inside this element must NOT activate any block — the
     * textarea keeps focus and handles the caret-move itself.
     *
     * Set to `null` when no editor is open. There is exactly one active
     * editor at a time, so a single shared ref is correct.
     */
    activeEditRegionRef?: React.MutableRefObject<HTMLDivElement | null>;
    /**
     * Globally disable the edit surface (bd-ov4gqk3m). When true, no
     * block renders an edit affordance (`data-block-pool-id`) and
     * `useBlockEditHover` is inert. Set by hosts that are read-only —
     * `q2 preview` without `--allow-edit`. Absent/false ⇒ editable.
     */
    editingDisabled?: boolean;
}

export const PreviewContext = createContext<PreviewContextValue | null>(null);

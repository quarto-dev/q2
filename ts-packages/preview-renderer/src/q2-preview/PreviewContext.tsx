import { createContext } from 'react';
import type React from 'react';
import type { BlockNode } from '../framework/types';
import type { ReachabilityClass, SourceIndexEntry, ResolvedSource } from './sourceIndex';
import type { MutableRefObject } from 'react';
import type { CaretHint } from './caretGeometry';

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
        /**
         * P3.3 nesting cursor: byte offset of the originally-clicked leaf — the
         * path bottom 'in' descends toward. Set at activation = anchorR0; mutated
         * by nesting-out/in in a later task.
         */
        leafAnchorR0?: number;
        /**
         * P3.3 nesting cursor: the value the draft was seeded with at open
         * (nestedEditBuffers[siKey] ?? anchorSlice). The dirty guard baselines
         * against THIS, not anchorSlice — a clean-buffer-seeded nested editor
         * differs from the polluted anchorSlice slice.
         */
        seededDraft?: string;
    } | null;
    /** Activate a block for editing, or pass null to clear. */
    setEditTarget?: (target: {
        anchorR0: number;
        anchorR1: number;
        anchorSlice: string;
        contentHeight: number;
        boxStyle: Record<string, string>;
        leafAnchorR0?: number;
        seededDraft?: string;
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
     * P2.3b: ref to the current `editTarget` state. Stable reference updated
     * every render. Used by `EditTextarea`'s `commitIfDirty` to guard against
     * writing a stale draft when the textarea unmounts due to a self-heal DROP
     * (or the transient unmount before a re-anchor):
     *
     *   - On a DROP, `editTargetRef.current` is set to null (by the self-heal
     *     effect's `setEditTargetRaw(null)`) before React unmounts the textarea.
     *     The `onBlur` fires during unmount with the stale draft — the guard
     *     checks `editTargetRef.current` and suppresses the commit.
     *   - On a transient unmount (old `anchorR0` stops matching before re-anchor),
     *     `editTargetRef.current` holds the OLD anchorR0. The guard compares
     *     `anchorR0 === resolved.sourceEntry.r[0]` — old anchorR0 ≠ re-anchored
     *     anchorR0 → guard suppresses the commit, draft survives in `editDraftRef`
     *     to be restored when the textarea re-mounts.
     *
     * A `MutableRefObject` is always referentially stable — exposing it on the
     * context does NOT cause extra re-renders.
     */
    editTargetRef?: MutableRefObject<{
        anchorR0: number;
        anchorR1: number;
        anchorSlice: string;
        contentHeight: number;
        boxStyle: Record<string, string>;
        leafAnchorR0?: number;
        seededDraft?: string;
    } | null>;
    /**
     * Globally disable the edit surface (bd-ov4gqk3m). When true, no
     * block renders an edit affordance (`data-block-pool-id`) and
     * `useBlockEditHover` is inert. Set by hosts that are read-only —
     * `q2 preview` without `--allow-edit`. Absent/false ⇒ editable.
     */
    editingDisabled?: boolean;
    /**
     * P3.2: nesting-cursor mode for nested blocks. When true, nested
     * list/quote blocks are each separately editable (nesting cursor).
     * Default-off (undefined/false). Set by the hub-client
     * `unlockNestingCursor` preference or the SPA's `?nestingCursor=1`
     * boot URL query param.
     */
    unlockNestingCursor?: boolean;
    /**
     * §3: stable ref mirror of `unlockNestingCursor`, updated in PreviewRoot's
     * render body. The hover handlers (`onPointerMove`/`onPointerLeave`) are
     * `useCallback(…, [])` and would otherwise capture a stale render-0 value of
     * the scalar; reading `.current` gives the live preference without churning
     * the handler identity (latest-ref pattern, Reflection #11/#12).
     */
    unlockNestingCursorRef?: MutableRefObject<boolean | undefined>;
    /**
     * P3.2: per-siKey clean QMD buffers for nested blocks. Produced by
     * `regenerateNestedBuffers` (gated on `unlockNestingCursor`). Keyed
     * by `siKey = "0:<r0>-<r1>:0"`. Undefined when the flag is off or
     * inputs are unavailable (safe default: no nesting editing).
     */
    nestedEditBuffers?: Record<string, string>;
    /**
     * P2.4b: cross-surface arrow-nav move trigger.
     *
     * Called by `EditTextarea`'s `onKeyDown` when a bare ArrowDown/ArrowUp
     * fires on the edge of the visual content. The root-level implementation
     * in `entry.tsx` handles both synchronous hops (unmodified) and
     * async relands (modified, waits for commit re-render).
     *
     * @param direction  'down' or 'up'
     * @param exitColumn getLogicalColumn(ta) at the moment the arrow fires
     * @param draft      Current textarea value
     * @param isDirty    Whether draft !== anchorSlice (dirty guard)
     * @param sourceInfoJson  JSON.stringify(resolved.sourceEntry) for committing
     */
    requestMove?: (
        direction: 'down' | 'up',
        exitColumn: number,
        draft: string,
        isDirty: boolean,
    ) => void;
    /**
     * P2.4b: pending caret hint. Set by the hop/reland just before opening
     * the destination editor; consumed (and cleared) by `EditTextarea`'s
     * mount effect to place the caret on the correct edge/column.
     *
     * The ref is root-held so it survives across React renders without
     * causing extra re-renders.
     */
    pendingCaretRef?: MutableRefObject<CaretHint | null>;
    /**
     * P2.4b: cancel a pending land (if any). Called by `EditTextarea`'s
     * Escape handler so Esc during a modified move that has not yet relanded
     * (waiting for the commit re-render or the byte-identical timeout) discards
     * the stashed landing instead of relanding after Esc.
     *
     * Clears `pendingLandingRef`, cancels the fallback timer, and clears
     * `pendingCaretRef`. No-op when no landing is pending.
     */
    cancelPendingLand?: () => void;
    /**
     * P2.4c: stash a plain-close focus landing. Called by `EditTextarea` BEFORE
     * `setEditTarget(null)` on Esc / Cmd-Enter / plain blur — NOT on a move
     * (moves go through `requestMove` which stashes intent:'open').
     *
     * After the editor closes, the next re-render (or the byte-identical timeout
     * fallback) focuses the outer block at `anchorR0` via `outerBlockForAnchorR0`, so
     * roving-tabindex resumes at the edited block.
     *
     * @param anchorR0  Byte offset of the outer block being closed.
     */
    requestFocusRestore?: (anchorR0: number) => void;
    /**
     * P2.4d: record a pending click-switch to outer block B.
     *
     * Called by `useBlockEditHover`'s `onPointerDown` when a MOUSE click lands
     * on a DIFFERENT outer block while an editor is open. Records the outer block element and
     * its current anchorR0 so the blur handler can project B's destination line
     * after A's potential commit.
     *
     * Must be called before A's blur fires (pointerdown precedes blur in the
     * browser event ordering). Clears any previously pending click-switch.
     *
     * @param blockEl  The resolved outer block B that was clicked.
     */
    requestClickSwitch?: (blockEl: Element) => void;
    /**
     * P2.4d: handle the dirty-or-unmodified click-switch in the blur handler.
     *
     * Called by `EditTextarea`'s `onBlur` INSTEAD OF `requestFocusRestore` when
     * a click-switch is pending. The implementation in `PreviewRoot.tsx` checks
     * whether the draft is dirty and branches:
     *
     *   - **Dirty:** commit A, stash a projected `pendingLanding` for B, suppress
     *     the normal focus-restore, mark `dirtySwitchHandled` so `onPointerUp`
     *     skips `activate(B)`. Returns `true` (consumed, blur handler must NOT
     *     call `requestFocusRestore` or `commitIfDirty`).
     *   - **Unmodified:** clear the click-switch record, let A close without a
     *     landing, let `onPointerUp`'s `activate(B)` proceed normally. Returns
     *     `false` (not consumed — blur handler still calls the normal
     *     `commitIfDirty` close, which is a no-op for unmodified drafts).
     *
     * Returns `false` when no click-switch is pending (normal blur path).
     *
     * @param draft            Current textarea value (raw, not yet normalized).
     * @param sourceInfoJson   JSON.stringify(resolved.sourceEntry) — same value
     *                         that `commitTextEdit` receives.
     */
    handleClickSwitchBlur?: (draft: string) => boolean;
    /**
     * P2.4d: check and clear the dirty-switch-handled flag.
     *
     * Called by `useBlockEditHover`'s `onPointerUp`. Returns `true` if a dirty
     * click-switch was handled in `onBlur` (so the landing for B is already
     * stashed and `activate(B)` must be suppressed). Clears the flag.
     */
    consumeDirtySwitchHandled?: () => boolean;
    /**
     * P3.3 §3c nested-block commit. Builds the destination from the LIVE
     * editTargetRef.current ({t:0,r:[anchorR0,anchorR1],d:0}); no-ops if null.
     * Used in unlockNestingCursor mode instead of the per-render-closure
     * commitTextEdit, so an unmount-time blur cannot write to a stale byte range.
     */
    commitNestingEdit?: (newText: string) => void;
    /**
     * P3.3 §3b: move the nesting cursor to the AST parent ('out') or the child
     * toward leafAnchorR0 ('in'); clamps at the ends (no parent → out no-ops;
     * cursor is a leaf → in no-ops); re-seeds the draft from the new node's
     * buffer/slice; no commit.
     */
    requestNestingMove?: (direction: 'in' | 'out') => void;
    /**
     * P3.4: jump the nesting cursor directly to the ancestor crumb at byte range
     * [r0, r1] (breadcrumb-click). Shares the re-target core with requestNestingMove;
     * leafAnchorR0 is unchanged so a later 'in' descends toward the original leaf.
     */
    requestNestingSelect?: (r0: number, r1: number) => void;
    /**
     * §7 expand-on-edit: root-held ref mirroring the expanded state of the
     * current editor. Set at EVERY open (both `activate` and `openEditTarget`):
     *   - keyboard roving activation → true
     *   - pointer/click/hop → false
     *
     * Reading `editExpandedRef.current` in `EditTextarea`'s `useState` initializer
     * ensures a self-heal remount sees the same value as the original open
     * (PRESERVED across remounts). The per-open write ensures a hop landing
     * reads the correct value (RESET on every new open, not left stale).
     *
     * Exposed on the context (like `editDraftRef` / `pendingCaretRef`) so
     * `EditTextarea` can read it without prop-drilling.
     */
    editExpandedRef?: MutableRefObject<boolean>;
    /**
     * §1 geometry snapshot: at activation, capture the rendered geometry of the
     * opened block's whole top-level subtree into PreviewRoot's editGeometryRef,
     * BEFORE the children are swapped to a textarea. `range` is the opened block's
     * `(anchorR0, anchorR1)` (used to derive the block-relative key origin). A
     * later nest move sizes its destination from this snapshot. No-op in locked
     * mode or for a flat (non-multilevel) block.
     */
    captureGeometry?: (openedEl: Element, range: { r0: number; r1: number }) => void;
}

export const PreviewContext = createContext<PreviewContextValue | null>(null);

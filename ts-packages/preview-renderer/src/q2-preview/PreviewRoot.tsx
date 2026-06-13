/**
 * PreviewRoot: the React component that drives q2-preview's iframe render.
 *
 * Extracted from `entry.tsx` so tests can mount it directly without
 * importing the module-top side effects (Bootstrap injection, message
 * listener, `window.__Q2_PREVIEW_RENDERER__`).
 *
 * `entry.tsx` imports this file and re-exports the public interface;
 * the production render path is unchanged.
 */

import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
    Ast,
    extractMetaString,
    CurrentActorContext,
} from '../framework';
import type { FormatRegistry, NoteInline, PandocAST } from '../framework';
import type { BlockNode } from '../framework/types';
import type { PreviewNodeEditPayload } from '../types/diagnostic';
import { previewRegistry, PreviewContext } from '.';
import { buildSourceIndex, serializeSourceEntry } from './sourceIndex';
import type { ResolvedSource } from './sourceIndex';
import { tileForAnchorR0, findReanchorCandidate, enumerateLockedTiles, captureEditTarget, measureTileBox } from './lockedTiles';
import { buildByteLineMap } from '../utils/byteLineMap';
import { normalizeLineEndings } from '../utils/normalizeLineEndings';
import { AssetManifestContext } from './AssetManifestContext';
import { NoteNumberingContext } from './NoteNumberingContext';
import { RevealDeck } from './RevealDeck';
import { installLinkHandlers } from '../utils/iframeLinkHandlers';
import { buildDepthCommitDestination, buildDepthSurfaces, parentSurface, childSurfaceToward } from './depthNav';
import { sliceBytes } from '../utils/sliceSource';

/**
 * P2.4b/c: descriptor for a pending cross-surface nav landing.
 * Stashed in `pendingLandingRef` when:
 *   - (P2.4b) a modified move fires — intent:'activate' opens the destination editor.
 *   - (P2.4c) a plain close fires — intent:'focus' returns focus to the edited tile.
 * Consumed by the reland layout effect after the commit re-render delivers new props,
 * or by the byte-identical timeout fallback.
 */
export type PendingLanding =
    | {
          /** P2.4b: open the destination editor on landing. */
          intent: 'activate';
          direction: 'down' | 'up';
          /** Target line for destination lookup in the new DOM/content. */
          destLine: number;
          /** Logical column to place the caret at on arrival. */
          desiredColumn: number;
          /** File that was active when the move fired — cancels on file switch. */
          fromFile: string;
      }
    | {
          /**
           * P2.4c: focus the edited tile on landing (plain close: Esc / Cmd-Enter / blur).
           * Does NOT open an editor — just positions roving-tabindex focus.
           */
          intent: 'focus';
          /** Byte offset of the edited tile — resolved via tileForAnchorR0. */
          anchorR0: number;
          /** File that was active when the close fired — cancels on file switch. */
          fromFile: string;
      };

/**
 * P2.4d: stashed at pointerdown time when a mouse click lands on a different tile
 * while an editor is open. Consumed by handleClickSwitchBlur.
 */
export interface ClickSwitchRecord {
    tileEl: Element;
    /** B's anchorR0 at pointerdown time (pre-commit, for projection). */
    anchorR0: number;
}

export interface PreviewRootProps {
    astJson: string;
    currentFilePath: string;
    /** Forwarded from `UPDATE_AST` payload; default is empty manifest. */
    assetManifest: Record<string, string>;
    /** Phase F.1: forwarded into `installLinkHandlers`. */
    projectFilePaths?: readonly string[];
    /** Phase F.1: post-render scroll target (no leading `#`). */
    pendingAnchor?: string | null;
    /** Phase F.1: monotonic epoch — scroll fires when this advances. */
    pendingAnchorEpoch?: number;
    /**
     * Reactji-authorship demo (2026-05-25 plan): viewer's Automerge
     * actor id, provided via `CurrentActorContext` to user TSX.
     */
    currentActor?: string | null;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
    renderedContent?: string;
    /** Pre-pipeline AST JSON for the structural editability gate (Plan 2a). */
    untransformedAstJson?: string | null;
    /** Globally disable the edit surface (bd-ov4gqk3m). */
    editingDisabled?: boolean;
    /**
     * P3.2: depth-cursor mode for nested blocks. When true, the context
     * exposes depth-cursor behaviour. Default-off (undefined/false).
     */
    unlockDepthCursor?: boolean;
    /**
     * P3.2: per-siKey clean QMD buffers for nested blocks, produced by
     * the host's `regenerateNestedBuffers` call. Undefined when off.
     */
    nestedEditBuffers?: Record<string, string>;
    /**
     * Optional user-registry overrides to merge on top of `previewRegistry`.
     * In production, `entry.tsx` passes `customRegistry` (loaded via
     * `LOAD_CUSTOM_COMPONENTS`). Tests pass `{}` (the default).
     */
    customRegistry?: Record<string, React.ComponentType<any>>;
    /**
     * Optional callback called when the document format switches to/from
     * slides. In production, `entry.tsx` passes `setDocIsSlides` to
     * reconcile the Bootstrap CSS link. Tests can ignore it (default no-op).
     */
    onDocIsSlides?: (isSlides: boolean) => void;
    /**
     * Optional callback called when `scrollToAnchorInDocument` would be called.
     * In production, `entry.tsx` provides the real scroll logic.
     * Tests can ignore it (default no-op).
     */
    scrollToAnchor?: (anchor: string) => boolean;
}

/**
 * Walk the parsed AST for `Note` inlines and assign each a sequential
 * number by document order. Lookup keyed by object identity — the
 * framework's walker-purity contract preserves Note references through
 * `unwrapCustomNodes`, so the WeakMap built here works on both pre-
 * and post-unwrap AST shapes.
 *
 * Walks pre-unwrap (over the JSON.parse output) so the same parsed
 * object can be handed to <Ast> via the discriminated input — avoids
 * a double-parse.
 *
 * Descends both `c` fields and CustomNode wrapper slot children
 * (`c[1][i].c[1]`) so notes nested inside callout / theorem bodies
 * are reached.
 */
function walkForNoteNumbers(ast: PandocAST): WeakMap<NoteInline, number> {
    const map = new WeakMap<NoteInline, number>();
    let counter = 0;
    function visit(value: unknown) {
        if (!value || typeof value !== 'object') return;
        if (Array.isArray(value)) {
            for (const v of value) visit(v);
            return;
        }
        const obj = value as { t?: unknown; c?: unknown };
        if (obj.t === 'Note') {
            counter += 1;
            map.set(value as NoteInline, counter);
        }
        if ('c' in obj) visit(obj.c);
    }
    visit(ast.blocks);
    return map;
}

/**
 * PreviewRoot: the root React component for q2-preview's iframe.
 *
 * Owns edit state, the P2.4b/c/d move/focus/click-switch machines,
 * and the P2.3b self-heal layout effect. Renders the AST via
 * `<Ast registry={mergedPreviewRegistry}>` inside `PreviewContext.Provider`
 * so all descendant blocks can read editing state and callbacks.
 */
export function PreviewRoot(props: PreviewRootProps) {
    const [editTarget, setEditTargetRaw] = useState<{
        anchorR0: number;
        anchorR1: number;
        anchorSlice: string;
        contentHeight: number;
        boxStyle: Record<string, string>;
        leafAnchorR0?: number;
        seededDraft?: string;
    } | null>(null);
    // Root-held ref for the in-flight edit draft. Seeded with anchorSlice at
    // activation; reset to null on close. Referentially stable → no extra
    // re-renders from draft changes.
    const editDraftRef = useRef<string | null>(null);
    // Ref pointing to the inner wrapper div of the currently active edit
    // region (set by renderMeasuredEdit in dispatchers.tsx). Used by
    // useBlockEditHover's onPointerUp to suppress the parent-climb bug:
    // a click inside this region must not activate any other surface.
    const activeEditRegionRef = useRef<HTMLDivElement | null>(null);
    const setEditTarget = useCallback((target: typeof editTarget | null) => {
        if (target !== null) {
            // Draft is seeded at the fresh-open site (activate / future nav-reland),
            // never here, so a P2.3b self-heal re-anchor via setEditTarget preserves
            // the in-flight draft without clobbering it.
            setEditTargetRaw(target);
        } else {
            editDraftRef.current = null;
            setEditTargetRaw(null);
            // P2.3b: drop-focus is handled in the self-heal layout effect below.
            // Plain-commit focus-restoration is deferred to P2.4 (pendingLanding).
        }
    }, []);

    // ---------------------------------------------------------------------------
    // P2.3b: self-heal + hidden-surface drop
    // ---------------------------------------------------------------------------

    // Keep editTarget in a ref so the layout effect reads it without depending on it.
    const editTargetRef = useRef<typeof editTarget>(editTarget);
    editTargetRef.current = editTarget;

    // previewHostRef: a ref to the wrapper div that scopes tile queries
    // (tileForAnchorR0) to the preview document tree.
    const previewHostRef = useRef<HTMLDivElement | null>(null);

    // Keep pool in a ref for the same reason — pool changes with astJson but we
    // don't want pool in the effect deps (that would fire on every render, not
    // just epoch-ticks). Pool is re-derived from astJson in the useMemo below.
    const poolRef = useRef<unknown[]>([]);
    // renderedContent ref: used by the P2.4b timeout fallback to read the latest
    // content without a stale closure over props.renderedContent.
    const renderedContentRef = useRef<string>(props.renderedContent ?? '');
    // P3.3 §3b: sourceIndexRef / nestedEditBuffersRef — stable refs so
    // requestDepthMove (a useCallback(fn, [])) can read the latest values.
    const sourceIndexRef = useRef<Map<string, { reachabilityClass: string }> | null | undefined>(null);
    const nestedEditBuffersRef = useRef<Record<string, string> | undefined>(undefined);

    // Self-heal layout effect. Fires post-DOM / pre-paint so re-anchoring is
    // invisible (no flicker). Keyed on the render inputs that signal an external
    // re-render: [astJson, renderedContent, untransformedAstJson].
    //
    // P2.3b fix: this effect answers ONE question — "did the active block survive
    // the edit?" — using pure pool/content logic (findReanchorCandidate). The
    // separate follow-up effect below answers the DOM question: "is the active
    // editor wrapper currently visible?"
    //
    // The old Step-2 `tileForAnchorR0(exactOnly:true)` check has been REMOVED.
    // While the editor is open, the active block's `<p data-block-pool-id="N">` is
    // replaced by the textarea wrapper div (which has NO `data-block-pool-id`).
    // tileForAnchorR0 only scans `[data-block-pool-id]` tiles, so it could NEVER
    // find the active editor → always returned null → always triggered a false DROP.
    // That caused KEEP to be unreachable in practice.
    useLayoutEffect(() => {
        const et = editTargetRef.current;
        if (et === null) return;  // no open editor — nothing to do

        const currentPool = poolRef.current;
        const currentContent = props.renderedContent ?? '';

        // Step 1: find a re-anchor candidate using content-verification.
        // KEEP: block survived (content unchanged, possibly shifted). Update offsets.
        // DROP: content mismatch or no candidate → close the editor.
        const cand = findReanchorCandidate(currentPool, currentContent, et.anchorR0, et.anchorSlice);

        if (cand) {
            // Re-anchor: update anchorR0/anchorR1; draft (editDraftRef) is untouched.
            // anchorSlice is unchanged (content-verified).
            const reanchored = { ...et, anchorR0: cand.r0, anchorR1: cand.r1 };
            setEditTargetRaw(reanchored);
        } else {
            // Drop — content mismatch or no candidate at/after anchorR0.
            editDraftRef.current = null;
            // P2.3b: explicitly null the ref BEFORE the drop-focus tile.focus() call.
            // tile.focus() causes the textarea to fire onBlur synchronously (focus moves
            // away). The onBlur handler calls commitIfDirty, which reads editTargetRef.current.
            // If the ref is still non-null at that point, the guard passes and the stale
            // draft is committed (Bug 2). Explicitly nulling the ref here ensures the guard
            // fires before tile.focus() triggers the blur.
            // Note: the render body also sets editTargetRef.current = editTarget, which will
            // re-null it after the re-render from setEditTargetRaw(null) — this is just an
            // early update that front-runs the re-render for guard correctness.
            editTargetRef.current = null;
            setEditTargetRaw(null);

            // Drop-focus: best-effort focus on the nearest visible tile at/after anchorR0.
            if (previewHostRef.current) {
                const tile = tileForAnchorR0(previewHostRef.current, currentPool, et.anchorR0);
                if (tile) {
                    (tile as HTMLElement).focus?.();
                }
            }
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [props.astJson, props.renderedContent, props.untransformedAstJson]);

    // Note: a follow-up "wrapper visibility" effect (keyed on editTarget) was
    // considered for the "active editor went hidden/collapsed → drop" case. It was
    // deferred because jsdom always returns zero rects (no layout engine), making
    // the wrapper-rect check unreliable in tests without per-test mocking of every
    // edit wrapper created during activation. The content-mismatch path in the
    // self-heal effect above handles the common case (collaborator edits the active
    // block). A genuine hidden-surface drop (same content, editor wrapper in a
    // display:none region) is not currently detected — deferred to a future pass
    // that adds DOM visibility checking with a jsdom-compatible approach.

    // ---------------------------------------------------------------------------
    // P2.4b: cross-surface arrow-nav move machine
    // ---------------------------------------------------------------------------

    const pendingLandingRef = useRef<PendingLanding | null>(null);
    const pendingCaretRef = useRef<{ edge: 'first' | 'last'; column: number } | null>(null);
    // Timer ID for the byte-identical fallback. Stored in a ref so Esc can cancel it.
    const fallbackTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    // Keep currentFilePath in a ref for use inside the move callbacks.
    const currentFilePathRef = useRef(props.currentFilePath);
    currentFilePathRef.current = props.currentFilePath;

    // Keep setAst in a ref so requestMove's useCallback(fn, []) does not capture
    // a stale closure. Pattern mirrors currentFilePathRef / onNavigateRef above.
    const setAstRef = useRef(props.setAst);
    setAstRef.current = props.setAst;

    /**
     * Execute a landing. Handles both intents:
     *
     * intent:'activate' (P2.4b move) — find the destination tile in the current
     * DOM/content and open its editor.
     *
     * intent:'focus' (P2.4c plain close) — focus the edited tile by anchorR0.
     */
    const executeLanding = useCallback((
        pl: PendingLanding,
        currentPool: unknown[],
        currentContent: string,
    ) => {
        // P2.4c: intent:'focus' — return focus to the edited tile without opening an editor.
        if (pl.intent === 'focus') {
            // Don't steal focus if a new edit has already started.
            if (editTargetRef.current !== null) {
                pendingLandingRef.current = null;
                return;
            }
            if (!previewHostRef.current) return;
            const tile = tileForAnchorR0(previewHostRef.current, currentPool, pl.anchorR0);
            if (tile) {
                (tile as HTMLElement).focus?.();
            }
            pendingLandingRef.current = null; // consumed
            return;
        }

        // intent:'activate' (P2.4b move): find the destination tile and open its editor.
        if (!previewHostRef.current) return;
        const tiles = enumerateLockedTiles(previewHostRef.current);
        if (tiles.length === 0) return;

        const map = buildByteLineMap(currentContent);
        let destTile: Element | null = null;

        if (pl.direction === 'down') {
            // First tile with start line >= destLine
            for (const tile of tiles) {
                const pidAttr = tile.getAttribute('data-block-pool-id');
                if (pidAttr === null) continue;
                const entry = currentPool[Number(pidAttr)] as { t: number; r: [number, number]; d: number } | undefined;
                if (!entry || entry.t !== 0) continue;
                if (map.lineOf(entry.r[0]) >= pl.destLine) { destTile = tile; break; }
            }
            if (!destTile) destTile = tiles[0]; // wrap to first
        } else {
            // Last tile with start line < destLine
            for (let i = tiles.length - 1; i >= 0; i--) {
                const tile = tiles[i];
                const pidAttr = tile.getAttribute('data-block-pool-id');
                if (pidAttr === null) continue;
                const entry = currentPool[Number(pidAttr)] as { t: number; r: [number, number]; d: number } | undefined;
                if (!entry || entry.t !== 0) continue;
                if (map.lineOf(entry.r[0]) < pl.destLine) { destTile = tile; break; }
            }
            if (!destTile) destTile = tiles[tiles.length - 1]; // wrap to last
        }

        if (!destTile) return;
        const captured = captureEditTarget(destTile, currentPool, currentContent);
        if (!captured) return;

        editDraftRef.current = captured.anchorSlice;
        pendingCaretRef.current = {
            edge: pl.direction === 'down' ? 'first' : 'last',
            column: pl.desiredColumn,
        };
        pendingLandingRef.current = null; // consumed
        // Measure the destination tile's box so the textarea renders at the
        // correct height (not collapsed to height: 0).
        const { contentHeight, boxStyle } = measureTileBox(destTile);
        setEditTargetRaw({
            ...captured,
            contentHeight,
            boxStyle,
        });
    }, []);

    /**
     * P2.4b reland layout effect.
     *
     * Fires when the render inputs change (after a commit round-trips).
     * Keyed on the same inputs as the self-heal effect.
     */
    useLayoutEffect(() => {
        const pl = pendingLandingRef.current;
        if (!pl) return; // no pending landing — nothing to do

        // File-switch cancellation.
        if (pl.fromFile !== currentFilePathRef.current) {
            pendingLandingRef.current = null;
            return;
        }

        executeLanding(pl, poolRef.current, renderedContentRef.current);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [props.astJson, props.renderedContent, props.untransformedAstJson]);

    /**
     * requestMove: called by EditTextarea's onKeyDown when a bare Arrow fires at
     * the visual edge of the textarea.
     */
    const requestMove = useCallback((
        direction: 'down' | 'up',
        exitColumn: number,
        draft: string,
        isDirty: boolean,
        sourceInfoJson: string,
    ) => {
        const et = editTargetRef.current;
        if (!et) return;

        const currentPool = poolRef.current;
        const currentContent = renderedContentRef.current;

        // Build the line map and compute the current tile's start line.
        const map = buildByteLineMap(currentContent);
        const L0 = map.lineOf(et.anchorR0);

        // Find the destination tile in the current DOM.
        if (!previewHostRef.current) return;
        const tiles = enumerateLockedTiles(previewHostRef.current);
        // Guard: empty → no-op. We allow tiles.length === 1 because the active
        // tile's DOM element has no data-block-pool-id (the textarea wrapper
        // replaced it), so a 2-tile document shows only 1 tile in the scan.
        // A single DOM tile + the active tile = at least 2 tiles total → nav is valid.
        if (tiles.length === 0) return;

        const draftLineCount = draft.split('\n').length;

        let destTile: Element | null = null;
        if (direction === 'down') {
            const targetLine = L0 + draftLineCount;
            for (const tile of tiles) {
                const pidAttr = tile.getAttribute('data-block-pool-id');
                if (pidAttr === null) continue;
                const entry = currentPool[Number(pidAttr)] as { t: number; r: [number, number]; d: number } | undefined;
                if (!entry || entry.t !== 0) continue;
                if (map.lineOf(entry.r[0]) >= targetLine) { destTile = tile; break; }
            }
            if (!destTile) destTile = tiles[0]; // wrap to first
        } else {
            for (let i = tiles.length - 1; i >= 0; i--) {
                const tile = tiles[i];
                const pidAttr = tile.getAttribute('data-block-pool-id');
                if (pidAttr === null) continue;
                const entry = currentPool[Number(pidAttr)] as { t: number; r: [number, number]; d: number } | undefined;
                if (!entry || entry.t !== 0) continue;
                if (map.lineOf(entry.r[0]) < L0) { destTile = tile; break; }
            }
            if (!destTile) destTile = tiles[tiles.length - 1]; // wrap to last
        }

        if (!isDirty) {
            // Synchronous hop: no commit, no editability gap.
            const captured = captureEditTarget(destTile!, currentPool, currentContent);
            if (!captured) return;
            editDraftRef.current = captured.anchorSlice;
            pendingCaretRef.current = {
                edge: direction === 'down' ? 'first' : 'last',
                column: exitColumn,
            };
            const { contentHeight, boxStyle } = measureTileBox(destTile!);
            setEditTargetRaw({
                ...captured,
                contentHeight,
                boxStyle,
            });
        } else {
            // Modified: compute destLine (delta-adjusted for down, unadjusted for up).
            const destLine = direction === 'down' ? L0 + draftLineCount : L0;
            pendingLandingRef.current = {
                intent: 'activate',
                direction,
                destLine,
                desiredColumn: exitColumn,
                fromFile: currentFilePathRef.current,
            };

            // Commit the edit and close the editor.
            const payload: PreviewNodeEditPayload = {
                __isPreviewNodeEdit: true,
                channel: 'text',
                destinationSourceInfoJson: sourceInfoJson,
                newText: normalizeLineEndings(draft),
            };
            setAstRef.current(payload as unknown as PandocAST);
            editDraftRef.current = null;
            setEditTargetRaw(null);

            // Byte-identical fallback.
            if (fallbackTimerRef.current !== null) {
                clearTimeout(fallbackTimerRef.current);
            }
            fallbackTimerRef.current = setTimeout(() => {
                fallbackTimerRef.current = null;
                const pl = pendingLandingRef.current;
                if (!pl) return;
                if (pl.fromFile !== currentFilePathRef.current) {
                    pendingLandingRef.current = null;
                    return;
                }
                executeLanding(pl, poolRef.current, renderedContentRef.current);
            }, 250);
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    /**
     * P2.4b: cancel a pending land (if any).
     */
    const cancelPendingLand = useCallback(() => {
        pendingLandingRef.current = null;
        pendingCaretRef.current = null;
        if (fallbackTimerRef.current !== null) {
            clearTimeout(fallbackTimerRef.current);
            fallbackTimerRef.current = null;
        }
    }, []);

    /**
     * P2.4c: stash a plain-close focus landing (intent:'focus') and arm the
     * byte-identical timeout fallback.
     */
    const requestFocusRestore = useCallback((anchorR0: number) => {
        // Cancel any prior pending land before stashing a new one.
        if (pendingLandingRef.current !== null) {
            pendingLandingRef.current = null;
            pendingCaretRef.current = null;
        }
        if (fallbackTimerRef.current !== null) {
            clearTimeout(fallbackTimerRef.current);
            fallbackTimerRef.current = null;
        }

        pendingLandingRef.current = {
            intent: 'focus',
            anchorR0,
            fromFile: currentFilePathRef.current,
        };

        // Arm timeout fallback.
        fallbackTimerRef.current = setTimeout(() => {
            fallbackTimerRef.current = null;
            const pl = pendingLandingRef.current;
            if (!pl) return;
            if (pl.fromFile !== currentFilePathRef.current) {
                pendingLandingRef.current = null;
                return;
            }
            executeLanding(pl, poolRef.current, renderedContentRef.current);
        }, 250);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    // ---------------------------------------------------------------------------
    // P2.4d: click-switch (pointerdown on a different tile while editing)
    // ---------------------------------------------------------------------------

    const clickSwitchRef = useRef<ClickSwitchRecord | null>(null);
    const dirtySwitchHandledRef = useRef<boolean>(false);

    /**
     * P2.4d: record a pending click-switch to tile B.
     */
    const requestClickSwitch = useCallback((tileEl: Element) => {
        const currentPool = poolRef.current;
        const pidAttr = tileEl.getAttribute('data-block-pool-id');
        if (!pidAttr) return;
        const entry = currentPool[Number(pidAttr)] as { t: number; r: [number, number]; d: number } | undefined;
        if (!entry || entry.t !== 0) return;
        clickSwitchRef.current = { tileEl, anchorR0: entry.r[0] };
        dirtySwitchHandledRef.current = false;
    }, []);

    /**
     * P2.4d: handle the click-switch case in the blur handler.
     *
     * Returns true if the click-switch was consumed (dirty case).
     * Returns false when no click-switch is pending (normal blur path proceeds).
     *
     * Projection: B is after A → destLine = L_B + delta (delta = draft lines − anchorSlice lines).
     *             B is before A → destLine = L_B, direction='down'
     *             (B's bytes are unaffected by A's edit; first-tile-at/after resolves exactly).
     */
    const handleClickSwitchBlur = useCallback((draft: string, sourceInfoJson: string): boolean => {
        const cs = clickSwitchRef.current;
        if (cs === null) return false; // no pending click-switch

        const et = editTargetRef.current;
        if (!et) {
            clickSwitchRef.current = null;
            return false;
        }

        const normalized = normalizeLineEndings(draft).trimEnd();
        const isDirty = !!normalized && normalized !== et.anchorSlice;

        if (!isDirty) {
            // Unmodified: clear click-switch; blur takes the normal commitIfDirty path.
            // onPointerUp's activate(B) proceeds as usual.
            clickSwitchRef.current = null;
            dirtySwitchHandledRef.current = false;
            return false;
        }

        // Dirty: commit A, stash pendingLanding for B, suppress focus-restore + pointerup activate.
        const currentContent = renderedContentRef.current;
        const map = buildByteLineMap(currentContent);
        const L_A = map.lineOf(et.anchorR0);
        const L_B = map.lineOf(cs.anchorR0);

        // Compute line delta from draft vs. original anchorSlice.
        const draftLineCount = normalizeLineEndings(draft).split('\n').length;
        const anchorSliceLineCount = et.anchorSlice.split('\n').length;
        const delta = draftLineCount - anchorSliceLineCount;

        // direction is always 'down':
        //   B >= A: project forward (destLine = L_B + delta, executeLanding finds first tile at line >= destLine).
        //   B < A:  B's bytes unchanged (destLine = L_B, first tile at line >= L_B = B itself).
        const direction = 'down';
        const destLine = L_B >= L_A ? L_B + delta : L_B;

        // Cancel any prior pending land before stashing B's landing.
        cancelPendingLand();

        pendingLandingRef.current = {
            intent: 'activate',
            direction,
            destLine,
            desiredColumn: 0,
            fromFile: currentFilePathRef.current,
        };

        // Arm the byte-identical timeout fallback (same pattern as requestMove).
        if (fallbackTimerRef.current !== null) clearTimeout(fallbackTimerRef.current);
        fallbackTimerRef.current = setTimeout(() => {
            fallbackTimerRef.current = null;
            const pl = pendingLandingRef.current;
            if (!pl) return;
            if (pl.fromFile !== currentFilePathRef.current) {
                pendingLandingRef.current = null;
                return;
            }
            executeLanding(pl, poolRef.current, renderedContentRef.current);
        }, 250);

        // Commit A (same wire format as requestMove's dirty path).
        const payload: PreviewNodeEditPayload = {
            __isPreviewNodeEdit: true,
            channel: 'text',
            destinationSourceInfoJson: sourceInfoJson,
            newText: normalizeLineEndings(draft),
        };
        setAstRef.current(payload as unknown as PandocAST);

        // Close the editor WITHOUT a focus-restore (landing will open B).
        editDraftRef.current = null;
        setEditTargetRaw(null);

        // Mark dirty switch handled so onPointerUp skips activate(B).
        dirtySwitchHandledRef.current = true;
        clickSwitchRef.current = null;

        return true; // consumed — blur handler must not also call requestFocusRestore/commitIfDirty
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [cancelPendingLand]);

    /**
     * P2.4d: check and clear the dirty-switch-handled flag.
     */
    const consumeDirtySwitchHandled = useCallback((): boolean => {
        if (dirtySwitchHandledRef.current) {
            dirtySwitchHandledRef.current = false;
            return true;
        }
        return false;
    }, []);

    /**
     * P3.3 §3c: nested-block commit. Builds the destination from the LIVE
     * editTargetRef.current so an unmount-time blur cannot write to a stale
     * byte range. No-ops when editTargetRef.current is null.
     */
    const commitDepthEdit = useCallback((newText: string) => {
        const dest = buildDepthCommitDestination(editTargetRef.current);
        if (dest === null) return; // no active target → no-op
        const payload: PreviewNodeEditPayload = {
            __isPreviewNodeEdit: true,
            channel: 'text',
            destinationSourceInfoJson: dest,
            newText: normalizeLineEndings(newText),
        };
        setAstRef.current(payload as unknown as PandocAST);
    }, []);

    /**
     * P3.3 §3b: move the depth cursor to the AST parent ('out') or the child
     * toward leafAnchorR0 ('in'). Clamps at the ends (no parent → out no-ops;
     * cursor is a leaf → in no-ops). Re-seeds the draft from the new node's
     * clean buffer (nestedEditBuffers[siKey] ?? anchorSlice). Does NOT commit.
     */
    const requestDepthMove = useCallback((direction: 'in' | 'out') => {
        const et = editTargetRef.current;
        if (!et) return;
        const surfaces = buildDepthSurfaces(sourceIndexRef.current);
        const next = direction === 'out'
            ? parentSurface(surfaces, et.anchorR0, et.anchorR1)
            : childSurfaceToward(surfaces, et.anchorR0, et.anchorR1, et.leafAnchorR0 ?? et.anchorR0);
        if (!next) return; // clamp — no-op at the path end
        const content = renderedContentRef.current;
        const anchorSlice = normalizeLineEndings(sliceBytes(content, next.r0, next.r1)).trimEnd();
        const siKey = serializeSourceEntry({ t: 0, r: [next.r0, next.r1], d: 0 });
        const seededDraft = nestedEditBuffersRef.current?.[siKey] ?? anchorSlice;
        editDraftRef.current = seededDraft;
        // Box: the parent ('out') is still a rendered tile → measure it.
        // The child ('in') is not in the DOM yet (editor is rendering there) →
        // fall back to the current box (best-effort; real box fidelity for 'in'
        // is a P3.4/Playwright concern).
        let contentHeight = et.contentHeight;
        let boxStyle = et.boxStyle;
        if (previewHostRef.current) {
            const tile = tileForAnchorR0(previewHostRef.current, poolRef.current, next.r0, { exactOnly: true });
            if (tile) {
                const m = measureTileBox(tile);
                contentHeight = m.contentHeight;
                boxStyle = m.boxStyle;
            }
        }
        pendingCaretRef.current = null; // depth move has no caret-edge hint
        setEditTargetRaw({
            anchorR0: next.r0,
            anchorR1: next.r1,
            anchorSlice,
            contentHeight,
            boxStyle,
            seededDraft,
            leafAnchorR0: et.leafAnchorR0 ?? et.anchorR0,
        });
    }, []);

    // Refs so the link-handler closure (installed once at mount)
    // sees the *latest* currentFilePath / projectFilePaths.
    const onNavigateRef = useRef(props.onNavigateToDocument);
    onNavigateRef.current = props.onNavigateToDocument;

    // Install link handlers once per mount.
    useEffect(() => {
        const scrollToAnchorInDocument = props.scrollToAnchor ?? (() => false);
        installLinkHandlers(document, {
            currentFilePath: props.currentFilePath,
            projectFilePaths: props.projectFilePaths,
            onQmdLinkClick: (arg) => {
                if ('path' in arg) {
                    if (arg.path === currentFilePathRef.current) {
                        if (arg.anchor) {
                            scrollToAnchorInDocument(arg.anchor);
                        }
                    } else {
                        onNavigateRef.current?.(arg.path, arg.anchor);
                    }
                } else {
                    scrollToAnchorInDocument(arg.anchor);
                }
            },
        });
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    // Phase F.1 (bd-kw93.14): scroll to the cross-page anchor after React commits the new AST.
    const lastScrolledEpochRef = useRef<number>(0);
    useEffect(() => {
        const epoch = props.pendingAnchorEpoch ?? 0;
        if (!props.pendingAnchor || epoch === 0) return;
        if (epoch === lastScrolledEpochRef.current) return;
        const anchor = props.pendingAnchor;
        const scrollToAnchorInDocument = props.scrollToAnchor ?? (() => false);
        const raf = requestAnimationFrame(() => {
            if (scrollToAnchorInDocument(anchor)) {
                lastScrolledEpochRef.current = epoch;
            }
        });
        return () => cancelAnimationFrame(raf);
    }, [props.pendingAnchor, props.pendingAnchorEpoch, props.astJson, props.scrollToAnchor]);

    // Single merge site: built-in preview leaves + user overrides.
    const mergedPreviewRegistry: FormatRegistry = {
        ...previewRegistry,
        ...(props.customRegistry ?? {}),
    } as FormatRegistry;

    // Pre-parse the AST and run the Note-numbering walk in one useMemo.
    const { parsed, noteNumbers, pool } = useMemo(() => {
        try {
            const raw = JSON.parse(props.astJson) as PandocAST & {
                astContext?: { p?: unknown[] };
            };
            return {
                parsed: raw as PandocAST,
                noteNumbers: walkForNoteNumbers(raw as PandocAST),
                pool: raw.astContext?.p ?? ([] as unknown[]),
            };
        } catch {
            return { parsed: null, noteNumbers: new WeakMap<NoteInline, number>(), pool: [] as unknown[] };
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [props.astJson]);

    // Keep poolRef / renderedContentRef in sync with the latest values. Updated in
    // render body (not in an effect) so layout effects always read current values.
    poolRef.current = pool;
    renderedContentRef.current = props.renderedContent ?? '';
    nestedEditBuffersRef.current = props.nestedEditBuffers;

    const astProps = parsed
        ? { ast: parsed }
        : { astJson: props.astJson };

    // `format: revealjs` previews as the `q2-slides` pseudo-format.
    const previewFormat = parsed ? extractMetaString(parsed.meta?.format) : undefined;
    const isSlides = previewFormat === 'q2-slides' || previewFormat === 'revealjs';

    // Notify caller (entry.tsx) about format switch so it can reconcile the Bootstrap CSS link.
    const onDocIsSlides = props.onDocIsSlides;
    useEffect(() => {
        onDocIsSlides?.(isSlides);
    }, [isSlides, onDocIsSlides]);

    // Build SourceInfo-value index from the untransformed AST (Plan 2a).
    const sourceIndex = useMemo(
        () => buildSourceIndex(props.untransformedAstJson),
        // eslint-disable-next-line react-hooks/exhaustive-deps
        [props.untransformedAstJson],
    );
    // P3.3 §3b: keep sourceIndexRef in sync (must come after sourceIndex useMemo).
    sourceIndexRef.current = sourceIndex;

    // resolveSource: look up a transformed block's untransformed counterpart via the SourceInfo value.
    const resolveSource = useCallback(
        (node: BlockNode): ResolvedSource | null => {
            if (!sourceIndex || !pool || pool.length === 0) return null;
            const s = (node as unknown as { s?: unknown }).s;
            if (s === undefined) return null;
            const sourceEntry = pool[Number(s)] as { t: 0; r: [number, number]; d: number } | undefined;
            if (!sourceEntry || sourceEntry.t !== 0) return null;
            const key = serializeSourceEntry(sourceEntry);
            const indexEntry = sourceIndex.get(key);
            if (!indexEntry) return null;
            return {
                sourceNode: indexEntry.sourceNode,
                reachabilityClass: indexEntry.reachabilityClass,
                sourceEntry,
            };
        },
        [pool, sourceIndex],
    );

    // commitTextEdit: send a text-channel PreviewNodeEditPayload (Plan 2b).
    const commitTextEdit = (destinationSourceInfoJson: string, newText: string) => {
        const payload: PreviewNodeEditPayload = {
            __isPreviewNodeEdit: true,
            channel: 'text',
            destinationSourceInfoJson,
            newText,
        };
        props.setAst(payload as unknown as PandocAST);
    };

    // commitSubtreeEdit: send a subtree-channel PreviewNodeEditPayload (Plan 2b).
    const commitSubtreeEdit = (destinationSourceInfoJson: string, modifiedBlock: BlockNode) => {
        const stripped = JSON.parse(JSON.stringify(modifiedBlock, (key, value) =>
            key === 's' || key === 'a' ? undefined : value,
        )) as BlockNode;
        const wrappedDoc = {
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [stripped],
        };
        const modifiedSubtreeJson = JSON.stringify(wrappedDoc);
        const payload: PreviewNodeEditPayload = {
            __isPreviewNodeEdit: true,
            channel: 'subtree',
            destinationSourceInfoJson,
            modifiedSubtreeJson,
        };
        props.setAst(payload as unknown as PandocAST);
    };

    return (
        <PreviewContext.Provider
            value={{
                currentFilePath: props.currentFilePath,
                pool,
                commitTextEdit,
                commitSubtreeEdit,
                content: props.renderedContent,
                editTarget,
                setEditTarget,
                editDraftRef,
                activeEditRegionRef,
                editTargetRef,
                sourceIndex,
                resolveSource,
                editingDisabled: props.editingDisabled,
                unlockDepthCursor: props.unlockDepthCursor,
                nestedEditBuffers: props.nestedEditBuffers,
                requestMove,
                pendingCaretRef,
                cancelPendingLand,
                requestFocusRestore,
                requestClickSwitch,
                handleClickSwitchBlur,
                consumeDirtySwitchHandled,
                commitDepthEdit,
                requestDepthMove,
            }}
        >
            {/* previewHostRef scopes tile queries (tileForAnchorR0) to the
                preview document so the self-heal effect does not accidentally
                query tiles from other parts of the page. The div is a
                transparent pass-through with no visual effect. */}
            <div ref={previewHostRef} style={{ display: 'contents' }}>
                <CurrentActorContext.Provider value={props.currentActor ?? null}>
                    <AssetManifestContext.Provider value={props.assetManifest}>
                        <NoteNumberingContext.Provider value={noteNumbers}>
                            {isSlides && parsed ? (
                                <RevealDeck
                                    ast={parsed}
                                    registry={mergedPreviewRegistry}
                                    currentFilePath={props.currentFilePath}
                                    onNavigateToDocument={props.onNavigateToDocument}
                                />
                            ) : (
                                <Ast
                                    {...astProps}
                                    currentFilePath={props.currentFilePath}
                                    onNavigateToDocument={props.onNavigateToDocument}
                                    setAst={props.setAst}
                                    registry={mergedPreviewRegistry}
                                />
                            )}
                        </NoteNumberingContext.Provider>
                    </AssetManifestContext.Provider>
                </CurrentActorContext.Provider>
            </div>
        </PreviewContext.Provider>
    );
}

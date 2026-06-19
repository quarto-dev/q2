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
import { outerBlockForAnchorR0, findReanchorCandidate, enumerateOuterBlocks, captureEditTarget, measureBlockBox, seedForRange, snapshotOuterBlockGeometry } from './outerBlocks';
import { buildByteLineMap } from '../utils/byteLineMap';
import { normalizeLineEndings } from '../utils/normalizeLineEndings';
import { AssetManifestContext } from './AssetManifestContext';
import { NoteNumberingContext } from './NoteNumberingContext';
import { RevealDeck } from './RevealDeck';
import { installLinkHandlers } from '../utils/iframeLinkHandlers';
import { buildNestingCommitDestination, buildNestingSurfaces, parentSurface, childSurfaceToward, childSurfaceTowardLine, topBlockR0, depthOfSurface, relocateSurface, surfaceAtLine } from './nestingNav';
import type { NestingSurface } from './nestingNav';
import type { CaretHint } from './caretGeometry';
import { prefixWidth } from './caretGeometry';

/**
 * §2 Phase 0: a landing-resolution spec — what the reland (or the synchronous
 * hop) should resolve the destination range/box from. `resolveLanding(spec)`
 * dispatches on `kind`. Today only `outerByLine` exists (the arrow-move /
 * click-switch find-by-line resolver, formerly copy-pasted between
 * `executeLanding` and `requestMove`); §2 adds `nest`/`crumb`.
 */
export type ResolverSpec =
    | {
          /** Find the destination outer block by source start line (arrow move / click switch). */
          kind: 'outerByLine';
          direction: 'down' | 'up';
          /** Target line: down → first outer block with startLine >= destLine; up → last with startLine < destLine. */
          destLine: number;
      }
    | {
          /**
           * §2 nest move (commit-if-dirty reland): relocate the committed
           * container by its commit-stable (startLine, depth) in the NEW source
           * index, then descend toward / ascend from the projected caret line.
           */
          kind: 'nest';
          direction: 'in' | 'out';
          /** `lineOf(et.anchorR0)` pre-commit — stable across an in-place list rewrite. */
          fromStartLine: number;
          /** Containment depth of the committed surface — disambiguates surfaces sharing a start line. */
          fromDepth: number;
          /** Caret's 0-based line within the pre-commit buffer (projected against the relocated node). */
          caretBufferLine: number;
          /** Caret's column within the pre-commit buffer (carried as the dest buffer column, best-effort). */
          caretBufferCol: number;
          /** Frozen leaf anchor — the no-readable-caret descent fallback. */
          leafAnchorR0: number;
      }
    | {
          /**
           * §2 crumb-jump (commit-if-dirty reland): relocate the chosen ancestor
           * by its commit-stable (startLine, depth), then place the caret by
           * projecting the pre-commit buffer line against the relocated target.
           */
          kind: 'crumb';
          /** Start line + containment depth of the chosen ancestor (depth disambiguates a shared start line). */
          targetStartLine: number;
          targetDepth: number;
          /** Start line of the surface being edited (for caret-line projection). */
          fromStartLine: number;
          caretBufferLine: number;
          caretBufferCol: number;
      };

/**
 * P2.4b/c: descriptor for a pending cross-surface nav landing.
 * Stashed in `pendingLandingRef` when:
 *   - (P2.4b) a modified move fires — intent:'open' opens the destination editor.
 *   - (P2.4c) a plain close fires — intent:'focus' returns focus to the edited tile.
 * Consumed by the reland layout effect after the commit re-render delivers new props,
 * or by the byte-identical timeout fallback.
 */
export type PendingLanding =
    | {
          /**
           * P2.4b: open the destination editor on landing. The destination is
           * resolved post-render via `resolveLanding(spec)`; `caret` is the
           * caret hint to place on arrival (null → caret at top).
           */
          intent: 'open';
          spec: ResolverSpec;
          caret: CaretHint | null;
          /** File that was active when the move fired — cancels on file switch. */
          fromFile: string;
      }
    | {
          /**
           * P2.4c: focus the edited tile on landing (plain close: Esc / Cmd-Enter / blur).
           * Does NOT open an editor — just positions roving-tabindex focus.
           */
          intent: 'focus';
          /** Byte offset of the edited outer block — resolved via outerBlockForAnchorR0. */
          anchorR0: number;
          /** File that was active when the close fired — cancels on file switch. */
          fromFile: string;
      };

// CaretHint (`{ edge } | { line }`, §2-widened) lives in caretGeometry.ts (the
// placement primitive's home); re-export it here for the landing core / context.
export type { CaretHint };

/**
 * P2.4d: stashed at pointerdown time when a mouse click lands on a different outer block
 * while an editor is open. Consumed by handleClickSwitchBlur.
 */
export interface ClickSwitchRecord {
    blockEl: Element;
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
     * P3.2: nesting-cursor mode for nested blocks. When true, the context
     * exposes nesting-cursor behaviour. Default-off (undefined/false).
     */
    unlockNestingCursor?: boolean;
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
            editGeometryRef.current = new Map(); // §1: plain close invalidates the snapshot
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

    // previewHostRef: a ref to the wrapper div that scopes outer-block queries
    // (outerBlockForAnchorR0) to the preview document tree.
    const previewHostRef = useRef<HTMLDivElement | null>(null);

    // Keep pool in a ref for the same reason — pool changes with astJson but we
    // don't want pool in the effect deps (that would fire on every render, not
    // just epoch-ticks). Pool is re-derived from astJson in the useMemo below.
    const poolRef = useRef<unknown[]>([]);
    // renderedContent ref: used by the P2.4b timeout fallback to read the latest
    // content without a stale closure over props.renderedContent.
    const renderedContentRef = useRef<string>(props.renderedContent ?? '');
    // P3.3 §3b: sourceIndexRef / nestedEditBuffersRef — stable refs so
    // requestNestingMove (a useCallback(fn, [])) can read the latest values.
    const sourceIndexRef = useRef<Map<string, { reachabilityClass: string }> | null | undefined>(null);
    const nestedEditBuffersRef = useRef<Record<string, string> | undefined>(undefined);
    // §1 geometry snapshot: rendered geometry of the active block's whole subtree,
    // captured at activation (pre-textarea-swap), keyed block-relative
    // ("${r0-topBlockR0}:${r1-topBlockR0}"). Consumed by nest moves to size their
    // destination from the original render. Each capture REPLACES the map; cleared
    // on plain close and on every self-heal fire (the re-anchor is content-based,
    // not a uniform δ, so block-relative keys can't be trusted across it).
    const editGeometryRef = useRef<Map<string, { contentHeight: number; boxStyle: Record<string, string> }>>(new Map());
    // Stable ref for the unlock-nesting preference so []-deps callbacks (capture
    // sites, future §3 hover handlers) read the latest value without re-subscribing.
    const unlockNestingCursorRef = useRef(props.unlockNestingCursor);
    unlockNestingCursorRef.current = props.unlockNestingCursor;

    // §1: capture the opened block's subtree geometry BEFORE the children are
    // swapped to a textarea. Gated to unlock mode (no nesting in locked mode) and,
    // implicitly, to multilevel blocks (snapshotOuterBlockGeometry returns empty
    // for a flat block). topBlockR0 is derived from the source index — the SAME
    // origin the consume-side lookup uses — so block-relative keys line up.
    const captureGeometry = useCallback((openedEl: Element, range: { r0: number; r1: number }) => {
        if (!unlockNestingCursorRef.current) {
            editGeometryRef.current = new Map();
            return;
        }
        const surfaces = buildNestingSurfaces(sourceIndexRef.current);
        const topR0 = topBlockR0(surfaces, range.r0, range.r1);
        editGeometryRef.current = snapshotOuterBlockGeometry(
            openedEl,
            poolRef.current as Array<{ t: number; r: [number, number]; d: number }>,
            topR0,
        );
    }, []);

    // Self-heal layout effect. Fires post-DOM / pre-paint so re-anchoring is
    // invisible (no flicker). Keyed on the render inputs that signal an external
    // re-render: [astJson, renderedContent, untransformedAstJson].
    //
    // P2.3b fix: this effect answers ONE question — "did the active block survive
    // the edit?" — using pure pool/content logic (findReanchorCandidate). The
    // separate follow-up effect below answers the DOM question: "is the active
    // editor wrapper currently visible?"
    //
    // The old Step-2 `outerBlockForAnchorR0(exactOnly:true)` check has been REMOVED.
    // While the editor is open, the active block's `<p data-block-pool-id="N">` is
    // replaced by the textarea wrapper div (which has NO `data-block-pool-id`).
    // outerBlockForAnchorR0 only scans `[data-block-pool-id]` outer blocks, so it could NEVER
    // find the active editor → always returned null → always triggered a false DROP.
    // That caused KEEP to be unreachable in practice.
    useLayoutEffect(() => {
        const et = editTargetRef.current;
        if (et === null) return;  // no open editor — nothing to do

        // §1: an external re-render invalidates the geometry snapshot on EVERY
        // self-heal fire (both KEEP and DROP). findReanchorCandidate is
        // content-based, not a uniform δ, so block-relative keys could describe the
        // wrong subtree; and KEEP spreads the old box forward without re-measuring.
        // Clear and let the next fresh open re-capture (consume falls back meanwhile).
        editGeometryRef.current = new Map();

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
            // P2.3b: explicitly null the ref BEFORE the drop-focus outerBlock.focus() call.
            // outerBlock.focus() causes the textarea to fire onBlur synchronously (focus moves
            // away). The onBlur handler calls commitIfDirty, which reads editTargetRef.current.
            // If the ref is still non-null at that point, the guard passes and the stale
            // draft is committed (Bug 2). Explicitly nulling the ref here ensures the guard
            // fires before outerBlock.focus() triggers the blur.
            // Note: the render body also sets editTargetRef.current = editTarget, which will
            // re-null it after the re-render from setEditTargetRaw(null) — this is just an
            // early update that front-runs the re-render for guard correctness.
            editTargetRef.current = null;
            setEditTargetRaw(null);

            // Drop-focus: best-effort focus on the nearest visible outer block at/after anchorR0.
            if (previewHostRef.current) {
                const outerBlock = outerBlockForAnchorR0(previewHostRef.current, currentPool, et.anchorR0);
                if (outerBlock) {
                    (outerBlock as HTMLElement).focus?.();
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
    const pendingCaretRef = useRef<CaretHint | null>(null);
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
     * Open (or re-open) the editor on a source range — the single canonical opener
     * shared by the three PreviewRoot-internal fresh-open / reland / nest-retarget
     * sites (executeLanding, requestMove's sync hop, applyNestingRetarget).
     * `activate` in useBlockEditHover does NOT route through this (it keeps its
     * mode-aware pre-open dedup local) — it shares only the pure `seedForRange`.
     *
     * Reads pool/content/buffers from refs (every internal caller already used the
     * same ref values), so this is a behavior-preserving extraction of the
     * previously-quadruplicated edit-target assembly.
     *
     *  - `box`: how to size the editor. `{ measure: el }` measures a live element;
     *    `'keep'` retains the current edit target's box; `'snapshot'` (§1) looks the
     *    destination up in `editGeometryRef` by its block-relative key, falling back
     *    to live-measure-if-rendered-else-keep when the snapshot lacks the key
     *    (jsdom/no-layout, an unrendered surface, or after a self-heal clear).
     *  - `caret`: caret-edge hint to place on arrival (null → caret at top).
     *  - `leafAnchorR0`: preserved across nest moves; defaults to `range.r0`.
     */
    const openEditTarget = useCallback((
        range: { r0: number; r1: number },
        opts: {
            box: { measure: Element } | 'keep' | 'snapshot';
            caret?: CaretHint | null;
            leafAnchorR0?: number;
        },
    ) => {
        const et = editTargetRef.current;
        const content = renderedContentRef.current;
        const { anchorSlice, seededDraft } = seedForRange(range, content, nestedEditBuffersRef.current);
        editDraftRef.current = seededDraft;

        let contentHeight: number;
        let boxStyle: Record<string, string>;
        if (opts.box === 'keep') {
            contentHeight = et?.contentHeight ?? 0;
            boxStyle = et?.boxStyle ?? {};
        } else if (opts.box === 'snapshot') {
            const surfaces = buildNestingSurfaces(sourceIndexRef.current);
            const topR0 = topBlockR0(surfaces, range.r0, range.r1);
            const snap = editGeometryRef.current.get(`${range.r0 - topR0}:${range.r1 - topR0}`);
            if (snap) {
                contentHeight = snap.contentHeight;
                boxStyle = snap.boxStyle;
            } else {
                // Fallback: measure the destination outer block if it is in the live
                // DOM (the pre-§1 best-effort), else keep the current box.
                let fb: { contentHeight: number; boxStyle: Record<string, string> } | null = null;
                if (previewHostRef.current) {
                    const ob = outerBlockForAnchorR0(previewHostRef.current, poolRef.current, range.r0, { exactOnly: true });
                    if (ob) fb = measureBlockBox(ob);
                }
                contentHeight = fb?.contentHeight ?? et?.contentHeight ?? 0;
                boxStyle = fb?.boxStyle ?? et?.boxStyle ?? {};
            }
        } else {
            const m = measureBlockBox(opts.box.measure);
            contentHeight = m.contentHeight;
            boxStyle = m.boxStyle;
        }

        pendingCaretRef.current = opts.caret ?? null;
        setEditTargetRaw({
            anchorR0: range.r0,
            anchorR1: range.r1,
            anchorSlice,
            contentHeight,
            boxStyle,
            seededDraft,
            leafAnchorR0: opts.leafAnchorR0 ?? range.r0,
        });
    }, []);

    /**
     * §2 Phase 0: the landing-resolution dispatcher. Given a `ResolverSpec`,
     * resolve the destination `{range, box}` (and optionally a `caret`) against
     * the CURRENT live DOM / content (read from refs). Returns null when no
     * destination can be resolved (no host, empty scan, uncapturable block).
     *
     * This is a PreviewRoot dispatcher, not a pure function: `outerByLine` reads
     * the live DOM via `enumerateOuterBlocks`; `nest` is pure-surface (refs only).
     * It collapses the find-by-line resolver that was previously copy-pasted
     * between `executeLanding` and `requestMove`. The box source is coupled to the
     * kind — `outerByLine` returns `{ measure: destBlock }` (a live element to size
     * from); `nest` returns `'snapshot'` + a `captureFrom` element (the freshly
     * re-rendered, clean destination subtree, captured by `openFromResolved`
     * before the open swaps it to a textarea).
     */
    const resolveLanding = useCallback((
        spec: ResolverSpec,
    ): { range: { r0: number; r1: number }; caret?: CaretHint; box: { measure: Element } | 'snapshot'; captureFrom?: Element } | null => {
        if (!previewHostRef.current) return null;
        const currentPool = poolRef.current;
        const currentContent = renderedContentRef.current;

        // ── §2 nest (commit-if-dirty reland) ─────────────────────────────────
        // Pure-surface: relocate the committed container by (startLine, depth) in
        // the NEW source index, project the caret line against it, then descend
        // (childSurfaceTowardLine) / ascend (parentSurface). box:'snapshot' — the
        // destination subtree is clean and rendered now, so openFromResolved
        // captures a fresh snapshot of it before opening (that capture IS its live
        // measure). Falls back to the byte-space leaf descent when there is no
        // projected caret span to aim at.
        if (spec.kind === 'nest') {
            const surfaces = buildNestingSurfaces(sourceIndexRef.current);
            if (surfaces.length === 0) return null;
            const map = buildByteLineMap(currentContent);
            const relocated = relocateSurface(surfaces, map, spec.fromStartLine, spec.fromDepth);
            if (!relocated) return null;
            const projectedLine = map.lineOf(relocated.r0) + spec.caretBufferLine;
            const next: NestingSurface | null =
                spec.direction === 'out'
                    ? parentSurface(surfaces, relocated.r0, relocated.r1)
                    : childSurfaceTowardLine(surfaces, relocated.r0, relocated.r1, projectedLine, map, currentContent)
                      ?? childSurfaceToward(surfaces, relocated.r0, relocated.r1, spec.leafAnchorR0);
            if (!next) return null; // clamp — no parent / leaf
            const destLine = Math.max(0, projectedLine - map.lineOf(next.r0));
            // §4 Principle A: route the column through prefixWidth, mirroring
            // cleanCaretHint. Compute the invariant source column Cs from the
            // from-surface's buffer column, then project it into the destination
            // surface's buffer column. Both surfaces go through prefixWidth so
            // clean and dirty paths share the same column semantics (Reflection #20).
            const curClean = nestedEditBuffersRef.current?.[
                serializeSourceEntry({ t: 0, r: [relocated.r0, relocated.r1], d: 0 })
            ];
            const destClean = nestedEditBuffersRef.current?.[
                serializeSourceEntry({ t: 0, r: [next.r0, next.r1], d: 0 })
            ];
            // projectedLine is the absolute source line here — same role as Ls in cleanCaretHint.
            const Cs = spec.caretBufferCol + prefixWidth(projectedLine, relocated.r0, currentContent, map, curClean);
            const destCol = Math.max(0, Cs - prefixWidth(projectedLine, next.r0, currentContent, map, destClean));
            // Same (bufferCol → Cs → destCol) projection as cleanCaretHint; factor after §6 lands.
            const caret: CaretHint = { line: destLine, column: destCol };
            // Element to snapshot the destination subtree from (its top block).
            const topR0 = topBlockR0(surfaces, next.r0, next.r1);
            const captureFrom =
                outerBlockForAnchorR0(previewHostRef.current, currentPool, topR0, { exactOnly: true }) ?? undefined;
            return { range: { r0: next.r0, r1: next.r1 }, caret, box: 'snapshot', captureFrom };
        }

        // ── §2 crumb (commit-if-dirty reland) ────────────────────────────────
        // Relocate the chosen ancestor by (startLine, depth) in the new index;
        // project the caret line against it. box:'snapshot' (clean re-rendered
        // subtree, captured by openFromResolved before opening).
        if (spec.kind === 'crumb') {
            const surfaces = buildNestingSurfaces(sourceIndexRef.current);
            if (surfaces.length === 0) return null;
            const map = buildByteLineMap(currentContent);
            const target = relocateSurface(surfaces, map, spec.targetStartLine, spec.targetDepth);
            if (!target) return null;
            const projectedLine = spec.fromStartLine + spec.caretBufferLine;
            const destLine = Math.max(0, projectedLine - map.lineOf(target.r0));
            const caret: CaretHint = { line: destLine, column: spec.caretBufferCol };
            const topR0 = topBlockR0(surfaces, target.r0, target.r1);
            const captureFrom =
                outerBlockForAnchorR0(previewHostRef.current, currentPool, topR0, { exactOnly: true }) ?? undefined;
            return { range: { r0: target.r0, r1: target.r1 }, caret, box: 'snapshot', captureFrom };
        }

        const map = buildByteLineMap(currentContent);

        // ── UNLOCK mode: surface-at-line navigation (Rule B, §1 C1/A2) ──────────
        // In unlock mode the active surface set is buildNestingSurfaces (the full
        // nesting partition including §0 list-item proxies). Navigate by finding
        // the innermost leaf at the target line; skip container-gap lines by
        // advancing L one step in the travel direction; clamp at document ends.
        if (unlockNestingCursorRef.current) {
            const surfaces = buildNestingSurfaces(sourceIndexRef.current);
            if (surfaces.length === 0) return null;

            // Determine the document line range from the surface set (min r0, max r1).
            // Bounds use raw byte spans (conservative); surfaceAtLine resolves via trimmed
            // spans, so trailing gap lines simply return null and are skipped by the loop below.
            let docStartLine = Infinity;
            let docEndLine = 0;
            for (const s of surfaces) {
                const [s0, s1] = [map.lineOf(s.r0), map.lineOf(Math.max(s.r0, s.r1 - 1))];
                if (s0 < docStartLine) docStartLine = s0;
                if (s1 > docEndLine) docEndLine = s1;
            }
            if (docStartLine === Infinity) return null;

            // Walk from the start line in the travel direction, skipping container-gap
            // lines (surfaceAtLine returns null), until we find a leaf or clamp.
            //
            // Down: start at spec.destLine (= L0 + draftLineCount, already past the
            //   current block's last line — correct starting point).
            // Up: start at spec.destLine - 1.
            //   For UP, spec.destLine = L0 (the CURRENT block's anchor line, NOT
            //   L0+draftLineCount as DOWN uses). Starting at spec.destLine would
            //   re-find the current surface via surfaceAtLine, so we subtract 1 to
            //   reach the line just above it.
            //   Do NOT "align" UP's start with DOWN's (i.e. L0+draftLineCount-1) —
            //   that would skip over blocks between L0 and L0+draftLineCount and
            //   break UP navigation entirely.
            const step = spec.direction === 'down' ? 1 : -1;
            let L = spec.direction === 'down' ? spec.destLine : spec.destLine - 1;
            let resolved = null;
            for (;;) {
                // Termination: L moves by ±1 each iteration; docStartLine/docEndLine are
                // finite bounds derived from the surface set, so the clamp break is always reached.
                if (L < docStartLine || L > docEndLine) break; // clamped
                resolved = surfaceAtLine(surfaces, L, currentContent, map);
                if (resolved !== null) break;
                L += step;
            }
            if (!resolved) return null; // clamped — no valid destination in this direction

            // Find the DOM element whose pool entry has r[0] === resolved.r0.
            // UNLOCK targets a potentially NESTED leaf surface (e.g. a Para inside a
            // BlockQuote, or a list item's leading block), so we search ALL pool elements
            // with querySelectorAll('[data-block-pool-id]'), not just outer blocks via
            // enumerateOuterBlocks (which only returns top-level blocks and would miss nested surfaces).
            if (!previewHostRef.current) return null;
            const allPoolEls = Array.from(
                previewHostRef.current.querySelectorAll<Element>('[data-block-pool-id]'),
            );
            let destEl: Element | null = null;
            for (const el of allPoolEls) {
                const pidAttr = el.getAttribute('data-block-pool-id');
                if (pidAttr === null) continue;
                const entry = currentPool[Number(pidAttr)] as { t: number; r: [number, number]; d: number } | undefined;
                if (!entry || entry.t !== 0) continue;
                if (entry.r[0] === resolved.r0) { destEl = el; break; }
            }
            if (!destEl) return null;

            return { range: { r0: resolved.r0, r1: resolved.r1 }, box: { measure: destEl } };
        }

        // ── LOCKED mode: outer-block navigation (clamp at ends, no wrap) ─────────
        const blocks = enumerateOuterBlocks(previewHostRef.current);
        if (blocks.length === 0) return null;

        let destBlock: Element | null = null;

        if (spec.direction === 'down') {
            // First outer block with start line >= destLine
            for (const outerBlock of blocks) {
                const pidAttr = outerBlock.getAttribute('data-block-pool-id');
                if (pidAttr === null) continue;
                const entry = currentPool[Number(pidAttr)] as { t: number; r: [number, number]; d: number } | undefined;
                if (!entry || entry.t !== 0) continue;
                if (map.lineOf(entry.r[0]) >= spec.destLine) { destBlock = outerBlock; break; }
            }
            // Clamp: no block found in the down direction → no-op (was: wrap to first)
        } else {
            // Last outer block with start line < destLine
            for (let i = blocks.length - 1; i >= 0; i--) {
                const outerBlock = blocks[i];
                const pidAttr = outerBlock.getAttribute('data-block-pool-id');
                if (pidAttr === null) continue;
                const entry = currentPool[Number(pidAttr)] as { t: number; r: [number, number]; d: number } | undefined;
                if (!entry || entry.t !== 0) continue;
                if (map.lineOf(entry.r[0]) < spec.destLine) { destBlock = outerBlock; break; }
            }
            // Clamp: no block found in the up direction → no-op (was: wrap to last)
        }

        if (!destBlock) return null;
        const captured = captureEditTarget(destBlock, currentPool, currentContent);
        if (!captured) return null;

        return { range: { r0: captured.anchorR0, r1: captured.anchorR1 }, box: { measure: destBlock } };
    }, []);

    /**
     * §2 Phase 0: open the editor from a resolved landing. When the resolver
     * sized the box from a live element (`{ measure }`), capture that subtree's
     * geometry FIRST (§1) — before `openEditTarget` swaps it to a textarea — so a
     * later nest move can size its destination from the original render. The
     * caret falls back to `fallbackCaret` when the resolver did not specify one
     * (e.g. `outerByLine`, whose column is producer-supplied).
     *
     * §2: a `nest` reland returns `box:'snapshot'` with a `captureFrom` element —
     * the freshly re-rendered, clean destination subtree. Snapshot it NOW (before
     * the open swaps it to a textarea) so the `box:'snapshot'` lookup hits.
     */
    const openFromResolved = useCallback((
        r: { range: { r0: number; r1: number }; caret?: CaretHint; box: { measure: Element } | 'snapshot'; captureFrom?: Element },
        fallbackCaret: CaretHint | null,
    ) => {
        if (r.box !== 'snapshot') {
            captureGeometry(r.box.measure, r.range);
        } else if (r.captureFrom) {
            captureGeometry(r.captureFrom, r.range);
        }
        openEditTarget(r.range, { box: r.box, caret: r.caret ?? fallbackCaret });
    }, [captureGeometry, openEditTarget]);

    /**
     * Execute a landing. Handles both intents:
     *
     * intent:'open' (P2.4b move) — resolve the destination via `resolveLanding`
     * against the current DOM/content and open its editor.
     *
     * intent:'focus' (P2.4c plain close) — focus the edited outer block by anchorR0.
     */
    const executeLanding = useCallback((
        pl: PendingLanding,
        currentPool: unknown[],
        _currentContent: string,
    ) => {
        // P2.4c: intent:'focus' — return focus to the edited outer block without opening an editor.
        if (pl.intent === 'focus') {
            // Don't steal focus if a new edit has already started.
            if (editTargetRef.current !== null) {
                pendingLandingRef.current = null;
                return;
            }
            if (!previewHostRef.current) return;
            const outerBlock = outerBlockForAnchorR0(previewHostRef.current, currentPool, pl.anchorR0);
            if (outerBlock) {
                (outerBlock as HTMLElement).focus?.();
            }
            pendingLandingRef.current = null; // consumed
            return;
        }

        // intent:'open' (P2.4b move): resolve the destination and open its editor.
        const r = resolveLanding(pl.spec);
        if (!r) return; // leave the pending landing for the fallback timer / next render
        pendingLandingRef.current = null; // consumed
        openFromResolved(r, pl.caret);
    }, [resolveLanding, openFromResolved]);

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
    ) => {
        const et = editTargetRef.current;
        if (!et) return;

        const currentContent = renderedContentRef.current;

        // Build the line map and compute the current outer block's start line.
        const map = buildByteLineMap(currentContent);
        const L0 = map.lineOf(et.anchorR0);

        const draftLineCount = draft.split('\n').length;
        // destLine asymmetry (pinned by the §2-Phase-0 characterization test,
        // Reflection #21): down → first outer block at/after L0+draftLineCount;
        // up → last outer block before L0 (NOT L0+draftLineCount). resolveLanding
        // compares `>= destLine` (down) / `< destLine` (up).
        const destLine = direction === 'down' ? L0 + draftLineCount : L0;
        const spec: ResolverSpec = { kind: 'outerByLine', direction, destLine };
        const caret: CaretHint = { edge: direction === 'down' ? 'first' : 'last', column: exitColumn };

        if (!isDirty) {
            // Synchronous hop: no commit, no editability gap. resolveLanding's
            // internal empty-scan guard returns null → no-op.
            const r = resolveLanding(spec);
            if (!r) return;
            openFromResolved(r, caret);
        } else {
            // Guard: empty scan → no destination to land on; skip the commit.
            // We allow blocks.length === 1 because the active outer block's DOM
            // element has no data-block-pool-id (the textarea wrapper replaced it),
            // so a 2-outer-block document shows only 1 outer block in the scan.
            if (!previewHostRef.current) return;
            if (enumerateOuterBlocks(previewHostRef.current).length === 0) return;

            // Modified: stash the landing spec; resolve post-commit against the new render.
            pendingLandingRef.current = {
                intent: 'open',
                spec,
                caret,
                fromFile: currentFilePathRef.current,
            };

            // Commit the edit and close the editor.
            // Use the LIVE identity from editTargetRef.current (via buildNestingCommitDestination)
            // rather than the per-render closure sourceInfoJson. For editable blocks (t=0, d=0)
            // the two are string-identical (see commit-destination-equivalence.test.ts), but the
            // live form anchors to the self-healed identity in every reachable commit state.
            const dest = buildNestingCommitDestination(editTargetRef.current);
            if (dest === null) {
                // No active target — skip commit and abort the move.
                editDraftRef.current = null;
                setEditTargetRaw(null);
                return;
            }
            const payload: PreviewNodeEditPayload = {
                __isPreviewNodeEdit: true,
                channel: 'text',
                destinationSourceInfoJson: dest,
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
    // P2.4d: click-switch (pointerdown on a different outer block while editing)
    // ---------------------------------------------------------------------------

    const clickSwitchRef = useRef<ClickSwitchRecord | null>(null);
    const dirtySwitchHandledRef = useRef<boolean>(false);

    /**
     * P2.4d: record a pending click-switch to outer block B.
     */
    const requestClickSwitch = useCallback((blockEl: Element) => {
        const currentPool = poolRef.current;
        const pidAttr = blockEl.getAttribute('data-block-pool-id');
        if (!pidAttr) return;
        const entry = currentPool[Number(pidAttr)] as { t: number; r: [number, number]; d: number } | undefined;
        if (!entry || entry.t !== 0) return;
        clickSwitchRef.current = { blockEl, anchorR0: entry.r[0] };
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
     *             (B's bytes are unaffected by A's edit; first-outer-block-at/after resolves exactly).
     */
    const handleClickSwitchBlur = useCallback((draft: string): boolean => {
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
        //   B >= A: project forward (destLine = L_B + delta, executeLanding finds first outer block at line >= destLine).
        //   B < A:  B's bytes unchanged (destLine = L_B, first outer block at line >= L_B = B itself).
        const direction = 'down';
        const destLine = L_B >= L_A ? L_B + delta : L_B;

        // Cancel any prior pending land before stashing B's landing.
        cancelPendingLand();

        pendingLandingRef.current = {
            intent: 'open',
            spec: { kind: 'outerByLine', direction, destLine },
            caret: { edge: direction === 'down' ? 'first' : 'last', column: 0 },
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
        // Use the LIVE identity from editTargetRef.current (via buildNestingCommitDestination)
        // rather than the per-render closure sourceInfoJson. At this point editTargetRef.current
        // is still A's identity (the editor is closing A, not B) — correct to use here.
        // For editable blocks (t=0, d=0) the two are string-identical
        // (see commit-destination-equivalence.test.ts), but the live form anchors to
        // the self-healed identity in every reachable commit state.
        const destA = buildNestingCommitDestination(editTargetRef.current);
        if (destA === null) {
            // No active target — skip commit and clean up.
            editDraftRef.current = null;
            setEditTargetRaw(null);
            dirtySwitchHandledRef.current = true;
            clickSwitchRef.current = null;
            return true;
        }
        const payload: PreviewNodeEditPayload = {
            __isPreviewNodeEdit: true,
            channel: 'text',
            destinationSourceInfoJson: destA,
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
    const commitNestingEdit = useCallback((newText: string) => {
        const dest = buildNestingCommitDestination(editTargetRef.current);
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
     * Shared re-target core (factored out of requestNestingMove). Re-seeds the
     * draft from the new node's clean buffer/slice and re-anchors editTarget,
     * sizing from the §1 geometry snapshot. No commit. The `caret` hint (computed
     * by the producer from the live caret) places the cursor in the destination.
     */
    const applyNestingRetarget = useCallback((next: { r0: number; r1: number }, leafAnchorR0: number, caret: CaretHint | null) => {
        const et = editTargetRef.current;
        if (!et) return;
        // §1: size the destination from the geometry snapshot captured at the
        // original open (the live DOM is edit-distorted — the active subtree is a
        // textarea — so even nest-out's parent is present-but-distorted). openEditTarget
        // resolves 'snapshot' to the block-relative key and falls back to
        // live-measure-if-rendered-else-keep on a miss.
        openEditTarget({ r0: next.r0, r1: next.r1 }, { box: 'snapshot', caret, leafAnchorR0 });
    }, []);

    /**
     * §2: read the live textarea's caret. Returns null when no editor textarea is
     * mounted (programmatic move). The selection survives the breadcrumb button's
     * `preventDefault`-on-pointerdown and the chord's own keydown, so this is the
     * ONE source of truth for both the chord (dispatchers) and the ▶/◀ buttons.
     * A non-collapsed selection uses its active end (`selectionEnd`, Reflection #18).
     */
    const readLiveCaret = useCallback((): { bufferLine: number; bufferCol: number; draft: string } | null => {
        const ta = activeEditRegionRef.current?.querySelector('textarea');
        if (!ta) return null;
        const value = ta.value;
        const pos =
            ta.selectionStart === ta.selectionEnd
                ? (ta.selectionStart ?? 0)
                : (ta.selectionEnd ?? ta.selectionStart ?? 0);
        const before = value.slice(0, pos);
        const lastNl = before.lastIndexOf('\n');
        const bufferLine = lastNl === -1 ? 0 : (before.match(/\n/g)?.length ?? 0);
        const bufferCol = pos - (lastNl + 1);
        return { bufferLine, bufferCol, draft: value };
    }, []);

    /**
     * §2: commit the live edit and stash a `kind:'nest'`/`'crumb'` landing, then
     * arm the byte-identical 250ms fallback. Shared by the dirty branches of
     * `requestNestingMove` and `requestNestingSelect` (the chord, ◀/▶ buttons, and
     * crumb-jumps all route through one commit-if-dirty chokepoint). The reland
     * (layout effect or this timer) resolves the spec against the new render.
     */
    const commitAndArmReland = useCallback((spec: ResolverSpec, draftSrc: string) => {
        const et = editTargetRef.current;
        if (!et) return;
        pendingLandingRef.current = {
            intent: 'open',
            spec,
            caret: null, // the nest/crumb resolver computes the dest caret post-relocation
            fromFile: currentFilePathRef.current,
        };
        const dest = buildNestingCommitDestination(et);
        if (dest === null) {
            pendingLandingRef.current = null;
            editDraftRef.current = null;
            setEditTargetRaw(null);
            return;
        }
        const payload: PreviewNodeEditPayload = {
            __isPreviewNodeEdit: true,
            channel: 'text',
            destinationSourceInfoJson: dest,
            newText: normalizeLineEndings(draftSrc),
        };
        setAstRef.current(payload as unknown as PandocAST);
        editDraftRef.current = null;
        setEditTargetRaw(null);

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
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [executeLanding]);

    /**
     * §2: the unified clean-move caret hint — map the live caret's invariant
     * source position (Ls,Cs) into the destination surface's buffer coords
     * (Reflection #20). Returns null when there is no readable caret. `prefixWidth`
     * bridges buffer columns ↔ source columns; a verbatim parent contributes 0.
     */
    const cleanCaretHint = useCallback((
        fromR0: number, fromR1: number, destR0: number, destR1: number,
        live: { bufferCol: number } | null, Ls: number | null,
        content: string, map: ReturnType<typeof buildByteLineMap>,
    ): CaretHint | null => {
        if (!live || Ls === null) return null;
        const curClean = nestedEditBuffersRef.current?.[serializeSourceEntry({ t: 0, r: [fromR0, fromR1], d: 0 })];
        const Cs = live.bufferCol + prefixWidth(Ls, fromR0, content, map, curClean);
        const destClean = nestedEditBuffersRef.current?.[serializeSourceEntry({ t: 0, r: [destR0, destR1], d: 0 })];
        return { line: Math.max(0, Ls - map.lineOf(destR0)), column: Math.max(0, Cs - prefixWidth(Ls, destR0, content, map, destClean)) };
    }, []);

    /**
     * §2: move the nesting cursor OUT to the AST parent or IN toward the live
     * CARET's source line (falling back to the frozen `leafAnchorR0` when there is
     * no readable caret). Clamps at the ends (no parent → out no-ops; leaf → in
     * no-ops). Commit-if-dirty: a CLEAN move hops synchronously (no commit); a
     * DIRTY move commits the edit, closes, and relands on the destination via the
     * `kind:'nest'` resolver — so no nest move discards an in-flight edit.
     */
    const requestNestingMove = useCallback((direction: 'in' | 'out') => {
        const et = editTargetRef.current;
        if (!et) return;
        // Re-entrancy guard (Reflection #22): one in-flight nest at a time — a
        // second request during the async commit→reland window would clobber the
        // pending landing or hit an unmounted textarea.
        if (pendingLandingRef.current !== null) return;

        const content = renderedContentRef.current;
        const map = buildByteLineMap(content);
        const surfaces = buildNestingSurfaces(sourceIndexRef.current);

        const live = readLiveCaret();
        const Ls = live ? map.lineOf(et.anchorR0) + live.bufferLine : null;

        // Dirty check (mirrors dispatchers' baseline: seededDraft ?? anchorSlice).
        const baseline = normalizeLineEndings(et.seededDraft ?? et.anchorSlice).trimEnd();
        const draftSrc = live ? live.draft : (editDraftRef.current ?? '');
        const draftNorm = normalizeLineEndings(draftSrc).trimEnd();
        const isDirty = !!draftNorm && draftNorm !== baseline;

        if (!isDirty) {
            // ── CLEAN sync hop: source unchanged, resolve + open synchronously ──
            const next = direction === 'out'
                ? parentSurface(surfaces, et.anchorR0, et.anchorR1)
                : (live && Ls !== null
                    ? childSurfaceTowardLine(surfaces, et.anchorR0, et.anchorR1, Ls, map, content)
                    : childSurfaceToward(surfaces, et.anchorR0, et.anchorR1, et.leafAnchorR0 ?? et.anchorR0));
            if (!next) return; // clamp — no-op at the path end

            // Unified in/out caret placement (Reflection #20): map the invariant
            // source position (Ls,Cs) into the destination surface's buffer coords.
            const caret = cleanCaretHint(et.anchorR0, et.anchorR1, next.r0, next.r1, live, Ls, content, map);
            applyNestingRetarget({ r0: next.r0, r1: next.r1 }, et.leafAnchorR0 ?? et.anchorR0, caret);
            return;
        }

        // ── DIRTY: commit-if-dirty, then reland on the destination (kind:'nest') ──
        // The committed container's start line + depth are commit-stable, so the
        // reland relocates it in the new source index and descends/ascends toward
        // the projected caret line. (Reflection #8.)
        if (!live) {
            // No live textarea to read the dirty draft from — should not happen
            // (a dirty buffer implies a mounted textarea); bail rather than guess.
            return;
        }
        commitAndArmReland({
            kind: 'nest',
            direction,
            fromStartLine: map.lineOf(et.anchorR0),
            fromDepth: depthOfSurface(surfaces, et.anchorR0, et.anchorR1),
            caretBufferLine: live.bufferLine,
            caretBufferCol: live.bufferCol,
            leafAnchorR0: et.leafAnchorR0 ?? et.anchorR0,
        }, draftSrc);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [readLiveCaret, cleanCaretHint, commitAndArmReland]);

    /**
     * P3.4 / §2: jump the nesting cursor directly to a chosen ancestor crumb's
     * range. Commit-if-dirty (the ▶/◀/crumb buttons preventDefault so the textarea
     * never blur-commits): a CLEAN jump hops synchronously (preserving the caret's
     * source position into the target); a DIRTY jump commits, then relands on the
     * target via `resolveLanding kind:'crumb'`.
     */
    const requestNestingSelect = useCallback((r0: number, r1: number) => {
        const et = editTargetRef.current;
        if (!et) return;
        if (pendingLandingRef.current !== null) return; // re-entrancy guard (Reflection #22)

        const content = renderedContentRef.current;
        const map = buildByteLineMap(content);
        const surfaces = buildNestingSurfaces(sourceIndexRef.current);

        const live = readLiveCaret();
        const Ls = live ? map.lineOf(et.anchorR0) + live.bufferLine : null;

        const baseline = normalizeLineEndings(et.seededDraft ?? et.anchorSlice).trimEnd();
        const draftSrc = live ? live.draft : (editDraftRef.current ?? '');
        const draftNorm = normalizeLineEndings(draftSrc).trimEnd();
        const isDirty = !!draftNorm && draftNorm !== baseline;

        if (!isDirty) {
            // CLEAN sync jump: preserve the caret's source position into the target.
            const caret = cleanCaretHint(et.anchorR0, et.anchorR1, r0, r1, live, Ls, content, map);
            applyNestingRetarget({ r0, r1 }, et.leafAnchorR0 ?? et.anchorR0, caret);
            return;
        }

        // DIRTY: commit, then reland on the chosen ancestor (relocated by
        // commit-stable (startLine, depth) to disambiguate a shared start line).
        if (!live) return;
        commitAndArmReland({
            kind: 'crumb',
            targetStartLine: map.lineOf(r0),
            targetDepth: depthOfSurface(surfaces, r0, r1),
            fromStartLine: map.lineOf(et.anchorR0),
            caretBufferLine: live.bufferLine,
            caretBufferCol: live.bufferCol,
        }, draftSrc);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [readLiveCaret, cleanCaretHint, commitAndArmReland]);

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
                unlockNestingCursor: props.unlockNestingCursor,
                unlockNestingCursorRef,
                nestedEditBuffers: props.nestedEditBuffers,
                requestMove,
                pendingCaretRef,
                cancelPendingLand,
                requestFocusRestore,
                requestClickSwitch,
                handleClickSwitchBlur,
                consumeDirtySwitchHandled,
                commitNestingEdit,
                requestNestingMove,
                requestNestingSelect,
                captureGeometry,
            }}
        >
            {/* previewHostRef scopes outer-block queries (outerBlockForAnchorR0) to the
                preview document so the self-heal effect does not accidentally
                query outer blocks from other parts of the page. The div is a
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

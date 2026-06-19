import React, { useContext, useState, useRef, useLayoutEffect } from 'react';
import { RegistryContext, AttributionWrap, renderChildren } from '../framework';
import type {
    BlockNode,
    CustomBlockNode,
    CustomInlineNode,
    InlineNode,
    NodeArgs,
} from '../framework';
import { PreviewContext } from './PreviewContext';
import type { PreviewContextValue, ResolvedSource } from './PreviewContext';
import { normalizeLineEndings } from '../utils/normalizeLineEndings';
import { isOnFirstVisualLine, isOnLastVisualLine, getLogicalColumn, placeCaretAtColumn } from './caretGeometry';
import { buildNestingCommitDestination, classifyNestingKey, detectPlatform } from './nestingNav';

// P3.3 §3b: detect platform once at module load so classifyNestingKey can
// distinguish mac (Cmd+Ctrl) from other (Alt+Shift) nesting chords.
const PLATFORM = detectPlatform();

/**
 * Block types whose left inset (the marker gutter: `<ul>`/`<ol>`/`<dl>`
 * padding-left) must NOT indent the editing textarea — their source markdown
 * starts at column 0. We drop only the *horizontal-left* box on these; the
 * vertical box (margins/padding/border top+bottom) is still replicated so
 * spacing around the block is preserved.
 */
const LEFT_INSET_STRIPPED_TYPES = new Set<string>([
    'BulletList', 'OrderedList', 'DefinitionList',
]);

/**
 * Measure-and-set edit wrapper — the single edit-mode rendering for every
 * editable block type.
 *
 * Reproduces the original element's full computed box — margin + padding +
 * per-side border — on a plain `<div>` so the textarea occupies exactly the
 * space (and shows the same decorations, e.g. an h2's Bootstrap `border-bottom`
 * rule) the replaced element did. `box-sizing: content-box` keeps the
 * textarea's `contentHeight` the content-area height, so the wrapper's total
 * border-box height equals the element's original height → zero reflow.
 *
 * (This replaced an earlier "wrap the original element and inject the textarea
 * inside it" hybrid. Reproducing the box on a synthetic div proved both
 * simpler and more faithful — it works uniformly for `<ul>`/`<table>` that
 * cannot legally contain a `<textarea>` and for `<p>`/`<h2>` alike.)
 */
function renderMeasuredEdit(
    node: BlockNode | CustomBlockNode,
    textarea: React.ReactNode,
    editTarget: NonNullable<PreviewContextValue['editTarget']>,
    activeEditRegionRef?: React.MutableRefObject<HTMLDivElement | null>,
): React.ReactNode {
    const wrapperStyle: React.CSSProperties = {
        ...(editTarget.boxStyle as React.CSSProperties),
        boxSizing: 'content-box',
    };
    // Lists carry a large left padding (the bullet/number gutter). Replicating
    // it would indent the textarea away from column 0 where the source begins;
    // drop the left inset for those types (vertical box is untouched).
    if (LEFT_INSET_STRIPPED_TYPES.has(node.t)) {
        wrapperStyle.paddingLeft = '0px';
        wrapperStyle.marginLeft = '0px';
        wrapperStyle.borderLeftWidth = '0px';
    }
    return (
        <AttributionWrap node={node} as="div">
            <div ref={activeEditRegionRef} id="q2-active-edit-region" style={wrapperStyle}>{textarea}</div>
        </AttributionWrap>
    );
}

const placeholderStyle: React.CSSProperties = {
    color: '#888',
    fontStyle: 'italic',
};

const PLACEHOLDER_CLASS = 'q2-preview-placeholder';

// ---------------------------------------------------------------------------
// Block editing helpers (Plan 3: centralized textarea substitution)
// ---------------------------------------------------------------------------

/**
 * C2-safe gate: true when this block is the active edit target.
 *
 * The `resolved != null` check is explicit — `null?.reachabilityClass !== 'Opaque'`
 * would evaluate true (undefined !== 'Opaque'), silently making Generated /
 * included-file blocks editable.  The widened gate includes both `TopLevel` and
 * `Descendable` (all non-Opaque, non-null classes).
 *
 * Identity is matched by `anchorR0` (the block's start byte offset captured at
 * click time) against `resolved.sourceEntry.r[0]`. This survives collaborator
 * inserts/removes above the active block — ordinals (poolId) shift but byte
 * ranges of existing blocks stay stable.
 */
function isBlockEditTarget(
    ctx: PreviewContextValue | null,
    resolved: ResolvedSource | null,
): boolean {
    return (
        resolved != null &&
        resolved.reachabilityClass !== 'Opaque' &&
        ctx?.commitTextEdit !== undefined &&
        ctx?.content != null &&
        ctx?.editTarget != null &&
        ctx.editTarget.anchorR0 === resolved.sourceEntry.r[0]
    );
}

/**
 * Controlled textarea for block editing. Local state isolates per-keystroke
 * re-renders to this component only. Seeded from `ctx.editDraftRef.current`
 * on mount so the draft survives remounts (e.g. when a collaborator inserts
 * a block above, shifting pool ordinals and forcing a key-change remount).
 *
 * Dirty guard: blur / Cmd-Enter only commit when
 * `normalizeLineEndings(draft).trimEnd() !== editTarget.anchorSlice`.
 * This makes CRLF-sourced blocks safe — the textarea LF-normalizes its value,
 * and `anchorSlice` was already normalized at activation time.
 *
 * IME composition: `isComposingRef` gates the blur handler so mid-composition
 * blurs (e.g. mobile keyboard dismiss before compositionend) do not commit.
 */
function EditTextarea({
    ctx,
    resolved,
}: {
    ctx: PreviewContextValue;
    resolved: ResolvedSource;
}) {
    const { contentHeight, anchorSlice } = ctx.editTarget!;
    // P3.3: the dirty baseline is the value the draft was seeded with at open
    // (nestedEditBuffers[siKey] ?? anchorSlice). For nested blocks seeded with
    // a clean buffer, anchorSlice carries '> '/indent and would incorrectly read
    // as "dirty" on an untouched clean-buffer editor. seededDraft is the true
    // baseline; fall back to anchorSlice when seededDraft is absent (non-nested
    // blocks, or pre-P3.3 activation paths).
    const baseline = normalizeLineEndings(ctx.editTarget!.seededDraft ?? anchorSlice).trimEnd();
    const isComposingRef = useRef(false);
    const taRef = useRef<HTMLTextAreaElement | null>(null);

    // Seed from the root-held ref so the draft persists across remounts.
    const [draft, setDraft] = useState<string>(
        () => ctx.editDraftRef?.current ?? baseline,
    );

    // P2.4b: caret placement on arrival.
    // When a pending-caret hint is set (by hop/reland), apply it once on mount
    // and clear the ref so subsequent re-mounts (e.g. self-heal) don't re-apply.
    // useLayoutEffect (not useEffect) so the caret is positioned before paint —
    // no single-frame flash of the caret at the default position after autoFocus.
    useLayoutEffect(() => {
        const ta = taRef.current;
        const hint = ctx.pendingCaretRef?.current;
        if (ta && hint) {
            placeCaretAtColumn(ta, hint);
            ctx.pendingCaretRef!.current = null; // consumed
        }
        // Only run on mount (empty deps). Geometry is only valid after the
        // textarea is attached to the DOM.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    const commitIfDirty = (text: string) => {
        // P2.3b (review hardening): guard against any action from a stale textarea.
        //
        // This guard is hoisted to the TOP of commitIfDirty — before the
        // empty/anchorSlice cancel check — so a stale/unmounting textarea does
        // NOTHING at all: neither cancels nor commits.
        //
        // Without hoisting, the cancel branch (draft === anchorSlice) could fire
        // from a stale textarea and call setEditTarget!(null), closing an editor
        // that a concurrent operation (e.g. self-heal re-anchor) just opened.
        //
        // EMPIRICAL REALITY (jsdom + Chromium fail-on-revert proven):
        //   React does NOT invoke onBlur on a component's prop when that component
        //   is unmounted by a re-render (collaborator-shift). The "stale teardown
        //   blur" from a re-render-driven unmount does NOT occur in either tier.
        //
        // The guard's ACTUAL load-bearing role:
        //   The self-heal DROP path calls tile.focus() to restore focus. That
        //   moves focus off a STILL-MOUNTED textarea → a real browser onBlur →
        //   commitIfDirty is called. At that point editTargetRef.current is null
        //   (cleared by setEditTargetRaw(null) in the DROP branch). The guard
        //   detects null → no-op, preventing a stale commit to the now-dropped
        //   block. (Tested in p2-3b-real.integration.test.tsx §4, fail-on-revert-proven.)
        //
        // Staleness is detected by comparing editTargetRef.current.anchorR0 (the
        // current active target) against resolved.sourceEntry.r[0] (this textarea's
        // r0 from its render closure):
        //   - DROP: editTargetRef.current is null → stale → no-op.
        //   - Active textarea: anchorR0 matches → proceed to cancel/commit logic.
        //
        // Guard is skipped when editTargetRef is not in context (e.g. legacy test
        // harnesses that provide a partial PreviewContextValue without editTargetRef).
        if (ctx.editTargetRef !== undefined) {
            const currentEt = ctx.editTargetRef.current;
            if (!currentEt || currentEt.anchorR0 !== resolved.sourceEntry.r[0]) {
                // This textarea is no longer the active target — do nothing.
                return;
            }
        }
        const normalized = normalizeLineEndings(text).trimEnd();
        // Cancel (close without commit) when the draft is empty/whitespace OR
        // equals the seeded baseline. An empty draft would delete the block —
        // the old pre-P2.3a code had an explicit empty guard, and we restore it here.
        // P3.3: baseline is seededDraft (or anchorSlice for non-nested blocks) so a
        // clean-buffer-seeded nested editor doesn't incorrectly commit on untouched blur.
        if (!normalized || normalized === baseline) {
            ctx.setEditTarget!(null);
            return;
        }
        // Self-heal-on-write hardening: build the commit destination from the LIVE
        // edit target (editTargetRef.current) via buildNestingCommitDestination. This is equivalent
        // to JSON.stringify(resolved.sourceEntry) for every editable block (t=0, d=0)
        // — proven by commit-destination-equivalence.test.ts — but anchors to the
        // self-healed identity rather than the per-render closure snapshot.
        //
        // When editTargetRef is absent (legacy test harnesses that provide a partial
        // PreviewContextValue), fall back to the closure form so existing tests pass.
        let dest: string;
        if (ctx.editTargetRef !== undefined) {
            const liveDest = buildNestingCommitDestination(ctx.editTargetRef.current);
            if (liveDest === null) {
                // Guard above passed but ref raced to null — skip.
                ctx.setEditTarget!(null);
                return;
            }
            dest = liveDest;
        } else {
            // Legacy harness fallback: closure form (string-identical to live form for
            // all editable blocks, see commit-destination-equivalence.test.ts).
            dest = JSON.stringify(resolved.sourceEntry);
        }
        ctx.commitTextEdit!(dest, normalizeLineEndings(text));
        ctx.setEditTarget!(null);
    };

    return (
        <textarea
            ref={taRef}
            autoFocus
            value={draft}
            style={{
                display: 'block',
                fontFamily: 'monospace',
                fontSize: '0.9em',
                width: '100%',
                height: contentHeight,
                boxSizing: 'border-box',
                resize: 'vertical',
            }}
            onChange={(e) => {
                const v = e.currentTarget.value;
                setDraft(v);
                // Keep the root ref in sync so remounts seed the latest draft.
                if (ctx.editDraftRef) ctx.editDraftRef.current = v;
            }}
            onCompositionStart={() => { isComposingRef.current = true; }}
            onCompositionEnd={() => { isComposingRef.current = false; }}
            onBlur={() => {
                // Do not commit mid-IME-composition (e.g. mobile dismiss before compositionend).
                if (isComposingRef.current) return;
                // P2.4d: check for a pending click-switch FIRST. If a dirty switch is in
                // progress, handleClickSwitchBlur commits A, stashes B's landing, and closes
                // the editor — all in one shot. Returns true when consumed (skip normal path).
                if (ctx.handleClickSwitchBlur?.(draft)) {
                    return; // dirty click-switch handled — do NOT also focus-restore or commitIfDirty
                }
                // P2.4c: stash focus restore BEFORE commitIfDirty closes the editor.
                // requestFocusRestore only stashes the landing + arms the timer;
                // setEditTarget(null) is called inside commitIfDirty.
                ctx.requestFocusRestore?.(ctx.editTarget!.anchorR0);
                commitIfDirty(draft);
            }}
            onKeyDown={(e) => {
                // P3.3 §3b: nesting-cursor navigation (only in unlockNestingCursor mode).
                // classifyNestingKey returns 'in'/'out' for the platform chord + ArrowRight/Left,
                // or null for anything else (bare arrows, modified arrows without the full
                // chord, unrecognised keys) — those fall through to the existing handlers.
                if (ctx.unlockNestingCursor && ctx.requestNestingMove) {
                    const dir = classifyNestingKey(e, PLATFORM);
                    if (dir) { e.preventDefault(); ctx.requestNestingMove(dir); return; }
                }
                if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                    e.preventDefault();
                    // P2.4c: stash focus restore BEFORE commitIfDirty closes the editor.
                    // requestFocusRestore only stashes; commitIfDirty handles commit + close.
                    ctx.requestFocusRestore?.(ctx.editTarget!.anchorR0);
                    commitIfDirty(draft);
                } else if (e.key === 'Escape') {
                    e.preventDefault();
                    // P2.4b: cancel any prior pending land so Esc during a dirty move
                    // (editor closed, waiting for commit re-render) does not reland.
                    // P2.4c: stash a focus-restore landing (Esc = no commit = no re-render,
                    // so the reland effect won't fire; the timeout fallback restores focus).
                    // cancelPendingLand is called first; requestFocusRestore re-arms the timer.
                    ctx.cancelPendingLand?.();
                    ctx.requestFocusRestore?.(ctx.editTarget!.anchorR0);
                    ctx.setEditTarget!(null);
                } else if (
                    // P2.4b: cross-surface arrow navigation.
                    // Only bare arrows (no Shift/Ctrl/Alt/Meta) trigger a move.
                    // Modified arrows (Shift+Arrow selects; Ctrl+Arrow jumps words) must
                    // be left to the native textarea — they must NOT leave the surface.
                    !e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey &&
                    ctx.requestMove &&
                    (e.key === 'ArrowDown' || e.key === 'ArrowUp')
                ) {
                    const ta = e.currentTarget as HTMLTextAreaElement;
                    const isDown = e.key === 'ArrowDown';
                    const onEdge = isDown
                        ? isOnLastVisualLine(ta)
                        : isOnFirstVisualLine(ta);

                    if (onEdge) {
                        e.preventDefault();
                        const exitColumn = getLogicalColumn(ta);
                        const normalized = normalizeLineEndings(draft).trimEnd();
                        // Empty/whitespace draft counts as not-dirty (cancel path),
                        // matching commitIfDirty's empty→cancel guard.
                        // P3.3: use baseline (= seededDraft ?? anchorSlice) for consistency
                        // with commitIfDirty's dirty check.
                        const isDirty = !!normalized && normalized !== baseline;
                        ctx.requestMove(
                            isDown ? 'down' : 'up',
                            exitColumn,
                            draft,
                            isDirty,
                        );
                        // Note: a move is NOT a plain close — requestFocusRestore is NOT
                        // called here. The move stashes intent:'open' via requestMove.
                    }
                    // If NOT on the edge, fall through — native caret move.
                }
            }}
        />
    );
}

/** Render an in-place editing textarea for the matched block. */
function renderBlockTextarea(
    ctx: PreviewContextValue,
    resolved: ResolvedSource,
): React.ReactNode {
    return <EditTextarea ctx={ctx} resolved={resolved} />;
}

// ---------------------------------------------------------------------------

/**
 * q2-preview's Block dispatcher. Looks up the format registry by Pandoc
 * tag and renders the corresponding leaf component, falling back to a
 * muted-gray "(not yet implemented)" placeholder when no component is
 * registered.
 *
 * **Textarea substitution (Plan 3):** when the C2-safe gate passes and this
 * block is the active edit target, renders a `<textarea>` instead of the leaf
 * component.  Centralizing here means all 16+ block components get editing
 * automatically without per-component textarea logic.
 *
 * The miss path **recurses into children via `renderChildren`** so
 * nested nodes also surface their own placeholders. Without recursion,
 * only top-level blocks would render and inline children of an
 * unrecognized block would be silently dropped — Plan 2A's empty
 * registry would leave the iframe visually empty under a Para. With
 * recursion the Goal section's "every node renders as a placeholder"
 * claim holds literally.
 *
 * The placeholder carries a `q2-preview-placeholder` class so the
 * smoke-fixture must-not-match selector (`div.q2-preview-placeholder`)
 * actually fires when the placeholder fires. The inline `style` is
 * preserved alongside (no theme-CSS dependency).
 *
 * Attribution wrap (Phase 3 of `2026-05-13-q2-preview-attribution.md`):
 * `AttributionWrap` paints the dispatched output with a `.q2-attr-wrap`
 * div carrying `data-sid` and inline `color` whenever this node has
 * resolved attribution; off-path it is a pass-through, so the
 * dispatcher output is byte-identical to pre-attribution.
 */
export const Block = (args: NodeArgs<BlockNode>) => {
    const { registry } = useContext(RegistryContext);
    const ctx = useContext(PreviewContext);
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;

    if (isBlockEditTarget(ctx, resolved) && ctx) {
        // Editing: replace the block with a measure-and-set textarea wrapper
        // that reproduces the element's exact box (see renderMeasuredEdit).
        // Pass activeEditRegionRef so the wrapper div is tracked — used by
        // useBlockEditHover's onPointerUp to suppress parent-climb activation.
        const textarea = renderBlockTextarea(ctx, resolved!);
        return renderMeasuredEdit(args.node, textarea, ctx.editTarget!, ctx.activeEditRegionRef);
    }

    const Component = registry[args.node.t];
    const inner = Component ? (
        <Component {...args} />
    ) : (
        <div className={PLACEHOLDER_CLASS} style={placeholderStyle}>
            {args.node.t} (not yet implemented){renderChildren(args)}
        </div>
    );

    return <AttributionWrap node={args.node} as="div">{inner}</AttributionWrap>;
};

/**
 * q2-preview's Inline dispatcher. Same pattern as `Block` for
 * inline-level nodes — placeholder + recursion on miss so nested
 * inlines surface their own placeholders. Also wraps in
 * `.q2-attr-wrap` when attribution is resolved.
 */
export const Inline = (args: NodeArgs<InlineNode>) => {
    const { registry } = useContext(RegistryContext);
    const Component = registry[args.node.t];
    const inner = Component ? (
        <Component {...args} />
    ) : (
        <span className={PLACEHOLDER_CLASS} style={placeholderStyle}>
            {args.node.t} (not yet implemented){renderChildren(args)}
        </span>
    );

    return <AttributionWrap node={args.node} as="span">{inner}</AttributionWrap>;
};

/**
 * q2-preview's CustomBlock dispatcher — sibling of `Block`. Looks up
 * the format registry by `node.type_name` (instead of `node.t`,
 * which is always the literal `'CustomBlock'`); falls back to the
 * `__fallback__` entry on miss.
 *
 * `previewRegistry` carries Pandoc-tag entries (`Para`, `Header`, ...)
 * AND CustomNode-type entries (`Callout`, `Theorem`, ...) under the
 * same key namespace; the two sets are disjoint by project policy
 * (locked at build time by `registry.test.ts`'s namespace-disjoint
 * assertion).
 *
 * Attribution wrap: same as `Block`. CustomNodes (Callout, Theorem,
 * FloatRefTarget, ...) cover larger source ranges than primitive
 * blocks, so attributing them paints the whole containing block in
 * the author's colour.
 */
export const CustomBlock = (args: NodeArgs<CustomBlockNode>) => {
    const { registry } = useContext(RegistryContext);
    const ctx = useContext(PreviewContext);
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;

    if (isBlockEditTarget(ctx, resolved) && ctx) {
        const textarea = renderBlockTextarea(ctx, resolved!);
        return renderMeasuredEdit(args.node, textarea, ctx.editTarget!, ctx.activeEditRegionRef);
    }

    const Component =
        registry[args.node.type_name] ?? registry['__fallback__'];
    return (
        <AttributionWrap node={args.node} as="div">
            <Component {...args} />
        </AttributionWrap>
    );
};

/**
 * q2-preview's CustomInline dispatcher. Same pattern as `CustomBlock`
 * for inline-level CustomNodes (`CrossrefResolvedRef`, `Equation`).
 */
export const CustomInline = (args: NodeArgs<CustomInlineNode>) => {
    const { registry } = useContext(RegistryContext);
    const Component =
        registry[args.node.type_name] ?? registry['__fallback__'];
    const inner = <Component {...args} />;

    return <AttributionWrap node={args.node} as="span">{inner}</AttributionWrap>;
};

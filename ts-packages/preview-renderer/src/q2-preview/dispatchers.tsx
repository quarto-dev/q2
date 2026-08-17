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
import { editBaseline } from './outerBlocks';
import { RichTextEditor } from './richtext/RichTextEditor';
import { EditToolbar } from './richtext/EditToolbar';
import { richTextAvailable, richEditorActiveForType } from './richTextSupport';

// P3.3 §3b: detect platform once at module load so classifyNestingKey can
// distinguish mac (Cmd+Ctrl) from other (Alt+Shift) nesting chords.
const PLATFORM = detectPlatform();

// G15: editor font size — extracted so the test can loosen its assertion when
// this value is tuned. Reduced from 0.9em to 0.825em so the monospace line box
// sits just below the rendered proportional line, preventing the first-keystroke
// grow (the residual after rows={1} removes the gross 2-line inflation).
const EDITOR_FONT_SIZE = '0.825em';

/** True when text, after line-ending normalization + trailing-whitespace removal,
 *  has no content — i.e. the block is effectively empty (the §6 delete trigger). */
function isEffectivelyEmpty(text: string): boolean {
    return normalizeLineEndings(text).trimEnd() === '';
}

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
    activeEditRegionRef: React.MutableRefObject<HTMLDivElement | null> | undefined,
    ctx: PreviewContextValue,
    richSupported: boolean,
    richActive: boolean,
): React.ReactNode {
    const wrapperStyle: React.CSSProperties = {
        ...(editTarget.boxStyle as React.CSSProperties),
        boxSizing: 'content-box',
        // Offset parent for the plain-surface toolbar's placement effect.
        position: 'relative',
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
            <div ref={activeEditRegionRef} id="q2-active-edit-region" style={wrapperStyle}>
                {/* Plain-surface toolbar: shown when rich/nesting is on, EXCEPT when
                    the rich editor is active (it mounts its own — `!richActive` keeps
                    exactly one). Code/CustomBlocks get it with just a type crumb. */}
                {(ctx.richText || ctx.unlockNestingCursor) && !richActive && (
                    <EditToolbar richSupported={richSupported} />
                )}
                {textarea}
            </div>
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
    const { contentHeight } = ctx.editTarget!;
    // G19: editBaseline is the SINGLE canonical source of the dirty baseline:
    // normalizeLineEndings(seededDraft ?? anchorSlice).trimEnd(). For nested blocks
    // seeded with a clean buffer, anchorSlice carries '> '/indent and would
    // incorrectly read as "dirty" on an untouched clean-buffer editor. Routing
    // through editBaseline ensures no site can drift to the raw slice again.
    const baseline = editBaseline(ctx.editTarget!);
    const isComposingRef = useRef(false);
    const taRef = useRef<HTMLTextAreaElement | null>(null);

    // Seed from the root-held ref so the draft persists across remounts.
    const [draft, setDraft] = useState<string>(
        () => ctx.editDraftRef?.current ?? baseline,
    );

    // §7 expand-on-edit: initialise from editExpandedRef so self-heal remounts
    // see the same value as the original open (PRESERVED across remounts by the ref).
    // The ref is set at every open, so the initializer is always correct.
    const [expanded, setExpanded] = useState<boolean>(
        () => ctx.editExpandedRef?.current ?? false,
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

    // §7 expand-on-edit: auto-size the textarea when expanded + control overflow-y.
    //
    // When `expanded`, set height to 'auto' (to shrink-wrap) then read scrollHeight
    // and clamp to max(contentHeight, scrollHeight). This fires on every draft/
    // expanded/contentHeight change — once expanded the textarea tracks text size,
    // growing AND shrinking, always clamped to the original content height floor.
    //
    // When NOT expanded, height stays at contentHeight (the inline style prop below).
    //
    // G12 — overflow-y clip rule (runs for BOTH expanded and collapsed states):
    // Set overflow-y to 'auto' only when the content is genuinely clipped
    // (scrollHeight > clientHeight + SCROLLJACK_TOLERANCE_PX). Otherwise use
    // 'hidden' so the wheel event passes through to the page (no scrolljack).
    //   - Collapsed, spilling:  overflowY = 'auto'  (internal scroll wanted)
    //   - Expanded or fitting:  overflowY = 'hidden' (wheel scrolls the page)
    // The 6px tolerance absorbs sub-pixel rounding + small leading/padding deltas.
    //
    // jsdom returns scrollHeight === 0 and clientHeight === 0 always, so max(ch,0)===ch
    // in tests — safe and non-shrinking. Pixel-fit assertions are e2e-only.
    const SCROLLJACK_TOLERANCE_PX = 6;
    useLayoutEffect(() => {
        const ta = taRef.current;
        if (!ta) return;
        if (expanded) {
            ta.style.height = 'auto';
            ta.style.height = `${Math.max(contentHeight, ta.scrollHeight)}px`;
        }
        // G12: set overflow-y based on whether content is genuinely clipped.
        const clips = ta.scrollHeight > ta.clientHeight + SCROLLJACK_TOLERANCE_PX;
        ta.style.overflowY = clips ? 'auto' : 'hidden';
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [draft, expanded, contentHeight]);

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
        // §6 three-way guard (uniform delete-by-emptying):
        //
        // Cancel (close without commit) when the draft equals the seeded baseline
        // (unchanged) OR when the draft was already empty and baseline is also empty
        // (nothing to delete — block had no content to remove).
        //
        // DELETE: when the draft is now empty but the baseline was non-empty, the
        // user has intentionally cleared a block → fall through to the commit path,
        // which calls commitTextEdit(dest, '') → backend deletes the block.
        //
        // P3.3: baseline is seededDraft (or anchorSlice for non-nested blocks) so a
        // clean-buffer-seeded nested editor doesn't incorrectly commit on untouched blur.
        if (normalized === baseline) {
            // Unchanged (including both non-empty unchanged, and empty-from-start untouched).
            ctx.setEditTarget!(null);
            return;
        }
        if (isEffectivelyEmpty(text) && !baseline) {
            // Already-empty block, draft still empty → nothing to delete → cancel.
            ctx.setEditTarget!(null);
            return;
        }
        // Remaining cases:
        //   - normalized !== baseline && !!normalized  → changed, non-empty → normal commit
        //   - !normalized && !!baseline               → had content, now empty → DELETE commit (empty text)
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
        // At this point the guards above have returned for (unchanged) and (already-empty).
        // So: empty draft here ⇒ baseline was non-empty ⇒ §6 DELETE (commit '').
        // The non-delete (real edit) path commits normalizeLineEndings(text) so both
        // an empty draft AND a whitespace-only draft (which trims to '') produce an
        // unambiguous newText==='' delete signal to the backend.
        let newText: string;
        if (!normalized) {
            newText = ''; // delete signal: draft emptied from non-empty baseline
        } else {
            newText = normalizeLineEndings(text);
        }
        ctx.commitTextEdit!(dest, newText);
        ctx.setEditTarget!(null);
    };

    return (
        <textarea
            ref={taRef}
            autoFocus
            // §7: data-expanded seam — present when expanded, absent when collapsed.
            // Tests assert expansion via this flag, never via pixel height.
            data-expanded={expanded ? '' : undefined}
            // G15: rows={1} sets the intrinsic height baseline to one line so the
            // §7 autosize grows FROM one line and never reads the HTML default rows=2
            // box (which would force a 2-line floor even for single-line content).
            rows={1}
            value={draft}
            style={{
                display: 'block',
                fontFamily: 'monospace',
                fontSize: EDITOR_FONT_SIZE,
                width: '100%',
                // When expanded the layout effect drives the height; the static prop
                // below is the initial height (contentHeight) before any expansion.
                height: contentHeight,
                boxSizing: 'border-box',
                resize: 'vertical',
            }}
            onClick={() => {
                // G11: a click inside the already-open editor (a "second click" after
                // activation) signals intent to read/edit the full content — expand.
                // Self-guarding: no-op when already expanded (idempotent).
                // The ACTIVATING click cannot trigger this because the textarea is not
                // yet mounted when the activating mousedown/mouseup are hit-tested
                // (setEditTarget routes through useState; the textarea mounts on the
                // next render). A genuine second click with both down+up on the
                // mounted textarea fires here.
                if (!expanded) {
                    setExpanded(true);
                    if (ctx.editExpandedRef) ctx.editExpandedRef.current = true;
                }
            }}
            onChange={(e) => {
                const v = e.currentTarget.value;
                setDraft(v);
                // Keep the root ref in sync so remounts seed the latest draft.
                if (ctx.editDraftRef) ctx.editDraftRef.current = v;
            }}
            onCompositionStart={() => { isComposingRef.current = true; }}
            onCompositionEnd={() => { isComposingRef.current = false; }}
            onBlur={(e) => {
                // Do not commit mid-IME-composition (e.g. mobile dismiss before compositionend).
                if (isComposingRef.current) return;
                // A rich/plain surface swap is not a commit (Phase 1a) — the swap
                // unmounts this textarea; its blur must not close the session.
                if (ctx.editorModeSwitchRef?.current) return;
                // P2.4d: check for a pending click-switch FIRST. If a dirty switch is in
                // progress, handleClickSwitchBlur commits A, stashes B's landing, and closes
                // the editor — all in one shot. Returns true when consumed (skip normal path).
                if (ctx.handleClickSwitchBlur?.(draft)) {
                    return; // dirty click-switch handled — do NOT also focus-restore or commitIfDirty
                }
                // Focus moved into UI that owns its focus (e.g. the
                // comment popup, marked [data-q2-owns-focus]) — commit,
                // but do NOT arm the focus-restore timer that would
                // steal focus back from it.
                if ((e.relatedTarget as Element | null)?.closest?.('[data-q2-owns-focus]')) {
                    commitIfDirty(draft);
                    return;
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
                    // §7: nesting chords are leave-keys — they do NOT expand.
                    if (dir) { e.preventDefault(); ctx.requestNestingMove(dir); return; }
                }

                // §7 expand-on-edit: determine whether this keystroke should expand the
                // surface. A keystroke expands ONLY when it stays in the surface:
                //   - NOT Cmd/Ctrl+Enter (commit chord — closes the editor)
                //   - NOT Esc (closes the editor)
                //   - NOT a bare edge arrow (cross-surface navigation)
                //   - NOT a nesting chord (handled above, already returned)
                //
                // For arrows: we must compute onEdge before the expand guard so we can
                // exclude edge arrows but include non-edge arrows (native caret move).
                // This share avoids a second isOnFirst/LastVisualLine call in the
                // cross-surface branch below.
                let arrowOnEdge = false;
                if (
                    !e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey &&
                    (e.key === 'ArrowDown' || e.key === 'ArrowUp')
                ) {
                    const ta = e.currentTarget as HTMLTextAreaElement;
                    const isDown = e.key === 'ArrowDown';
                    arrowOnEdge = isDown ? isOnLastVisualLine(ta) : isOnFirstVisualLine(ta);
                }

                const isCommitChord = (e.metaKey || e.ctrlKey) && e.key === 'Enter';
                const isEsc = e.key === 'Escape';
                const isLeaveKey = isCommitChord || isEsc || arrowOnEdge;

                // G4: bare modifier keydowns (the leading keydown before a chord)
                // must NOT expand the editor. Only bare modifier-alone keys are
                // excluded — modifier+printable (e.g. ⌘V paste) is NOT excluded
                // because those fall through to the native textarea and may need
                // to expand after the change event follows.
                // Note: each disjunct repeats `e.key ===`; `e.key === 'Shift' || 'Control'`
                // is an always-truthy bug — do NOT write that shorthand.
                const isBareModifier =
                    e.key === 'Shift' || e.key === 'Control' || e.key === 'Alt' || e.key === 'Meta';

                if (!isLeaveKey && !isBareModifier && !expanded) {
                    // In-surface keystroke (printable char, Backspace/Delete, Home/End,
                    // non-edge arrow, or any other key that stays in the surface).
                    // Expand the editor and mirror the state to the root ref so remounts
                    // read the same value (PRESERVED across self-heal re-anchors).
                    setExpanded(true);
                    if (ctx.editExpandedRef) ctx.editExpandedRef.current = true;
                }

                if (isCommitChord) {
                    e.preventDefault();
                    // P2.4c: stash focus restore BEFORE commitIfDirty closes the editor.
                    // requestFocusRestore only stashes; commitIfDirty handles commit + close.
                    ctx.requestFocusRestore?.(ctx.editTarget!.anchorR0);
                    commitIfDirty(draft);
                } else if (isEsc) {
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
                    // arrowOnEdge was computed above (shared with the expand guard).
                    if (arrowOnEdge) {
                        e.preventDefault();
                        const ta = e.currentTarget as HTMLTextAreaElement;
                        const exitColumn = getLogicalColumn(ta);
                        const normalized = normalizeLineEndings(draft).trimEnd();
                        // §6 dirty check for arrow-away (mirrors commitIfDirty's three-way guard):
                        // A block emptied from non-empty counts as dirty (delete path).
                        // An already-empty block that remains empty is NOT dirty (nothing changed).
                        // P3.3: use baseline (= seededDraft ?? anchorSlice) for consistency
                        // with commitIfDirty's dirty check.
                        const isDirty = normalized !== baseline;
                        ctx.requestMove(
                            e.key === 'ArrowDown' ? 'down' : 'up',
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

// `RICHTEXT_SUPPORTED_TYPES` / `richTextAvailable` live in the leaf module
// `richTextSupport.ts` (shared with the breadcrumb, no import cycle).

/**
 * Which edit surface to render for the active block: the tiptap editor when rich
 * text is available AND the session mode is 'rich' (the default); otherwise the
 * monospaced textarea (flag off, unsupported type, or the user toggled to plain).
 */
function renderBlockEditSurface(
    ctx: PreviewContextValue,
    resolved: ResolvedSource,
    sourceNodeType: string,
): React.ReactNode {
    // Read the SAME predicate the toolbar gate uses, so the surface choice and
    // `!richActive` can't drift — exactly-one-toolbar is structural, not coincidental.
    if (richEditorActiveForType(ctx, sourceNodeType)) {
        return <RichTextEditor ctx={ctx} resolved={resolved} />;
    }
    return renderBlockTextarea(ctx, resolved);
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
        // Editing: replace the block with a measure-and-set wrapper that
        // reproduces the element's exact box (see renderMeasuredEdit). The inner
        // surface is the rich-text editor (opt-in, paragraphs) or the textarea.
        // Pass activeEditRegionRef so the wrapper div is tracked — used by
        // useBlockEditHover's onPointerUp to suppress parent-climb activation.
        const sourceNodeType = (resolved!.sourceNode as { t: string }).t;
        // Compute once (sourceNodeType in scope) and thread into renderMeasuredEdit.
        const richActive = richEditorActiveForType(ctx, sourceNodeType);
        const surface = renderBlockEditSurface(ctx, resolved!, sourceNodeType);
        return renderMeasuredEdit(
            args.node, surface, ctx.editTarget!, ctx.activeEditRegionRef,
            ctx, richTextAvailable(ctx, sourceNodeType), richActive,
        );
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
        // CustomBlocks aren't rich-editable → textarea, no toggle (richSupported/richActive false).
        return renderMeasuredEdit(args.node, textarea, ctx.editTarget!, ctx.activeEditRegionRef, ctx, false, false);
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

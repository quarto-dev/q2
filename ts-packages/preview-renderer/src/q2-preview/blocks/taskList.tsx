import { createContext, useContext } from 'react';
import type { ReactNode } from 'react';
import { Node } from '../../framework';
import type { BlockNode, InlineNode } from '../../framework';
import type { PreviewContextValue } from '../PreviewContext';
import type { ResolvedSource } from '../sourceIndex';

/**
 * Task-list support (bd-obkvhlam / bd-tvtknbhx / bd-qif9l4cx).
 *
 * The reader parses `- [ ] todo` into Pandoc's convention: the item's first
 * inline is `Str "☐"` (unchecked) / `Str "☒"` (checked) followed by `Space`.
 * The native HTML writer rewrites the item's head `Plain`/`Para` inlines to
 * `<label><input type="checkbox" …/>…</label>`, so a tight item renders
 * `<li><label>…</label></li>` and a loose one `<li><p><label>…</label></p></li>`.
 * This module gives the q2-preview React renderer the same DOM, plus the
 * interactive half: in an edit-enabled surface, toggling a checkbox flips the
 * ballot-box `Str` in the list's *untransformed* source node and commits it
 * through the subtree edit channel — `apply_node_edit` + the qmd writer's
 * task-marker round-trip turn that into a `[ ]` ↔ `[x]` splice in the source.
 *
 * ## Where the `<label>` is built (bd-qif9l4cx)
 *
 * The `<label>` is rendered by the head block itself (`Plain`/`Para`), not
 * by the `<li>` around `<Node node={head}>`. A block dispatched through
 * `<Node>` picks up block-level decorations on the way — CommentBlock's
 * positioned wrapper, the attribution wrapper, the measured edit surface —
 * and any of those landing *inside* an inline `<label>` after the `<input>`
 * pushes the item text onto its own line (the reported bug). Building the
 * label inside the block keeps every wrapper an ancestor of the label, so
 * checkbox and text always share one inline formatting context. The `<li>`
 * hands the checked state and toggle handler to its head block through
 * `TaskItemContext`; the head block strips the marker and wraps the rest.
 */

/** What the `<li>` tells its head block: checked state, and the toggle
 * handler (absent = render a disabled checkbox). */
export interface TaskItemState {
    checked: boolean;
    onToggle?: () => void;
}

/** Provided by `BulletList`/`OrderedList` around a task item's head block
 * only. `null` everywhere else, including inside the label's own content. */
export const TaskItemContext = createContext<TaskItemState | null>(null);

function isTaskMarker(inline: InlineNode | undefined): inline is InlineNode & { t: 'Str'; c: '☐' | '☒' } {
    return !!inline && inline.t === 'Str' && (inline.c === '☐' || inline.c === '☒');
}

/** `true`/`false` = task item with that checked state; `null` = not a task
 * item. Mirrors the writer's `task_item_checked`: the head block is a
 * `Plain` (tight) or `Para` (loose) whose inlines start with the marker
 * `Str` followed by `Space`. */
export function taskItemChecked(item: BlockNode[]): boolean | null {
    const head = item[0] as any;
    if (!head || (head.t !== 'Plain' && head.t !== 'Para') || !Array.isArray(head.c)) return null;
    const first = head.c[0];
    if (!isTaskMarker(first)) return null;
    if (head.c[1]?.t !== 'Space') return null;
    return first.c === '☒';
}

/** All items are task items (and there is at least one). Drives `class="task-list"`
 * on `<ul>` — bullet lists only, matching Pandoc's HTML writer. */
export function allTaskItems(items: BlockNode[][]): boolean {
    return items.length > 0 && items.every((item) => taskItemChecked(item) !== null);
}

/** The inlines after the marker `Str` + `Space`, or `null` when `inlines`
 * does not start with a task marker. Used by the head block to decide
 * whether it is the task head and what goes inside the `<label>`. */
export function stripTaskMarker(inlines: InlineNode[]): InlineNode[] | null {
    if (!isTaskMarker(inlines[0]) || inlines[1]?.t !== 'Space') return null;
    return inlines.slice(2);
}

/**
 * Build the toggle handler for item `itemIndex` of a list whose resolved
 * source is `resolved`. Returns `undefined` when the surface cannot commit
 * (no context, editing disabled, unresolvable/opaque source) — callers render
 * a disabled checkbox in that case.
 */
export function makeTaskToggle(
    ctx: PreviewContextValue | null | undefined,
    resolved: ResolvedSource | null,
    itemIndex: number,
): (() => void) | undefined {
    if (!ctx?.commitSubtreeEdit || ctx.editingDisabled) return undefined;
    if (!resolved || resolved.reachabilityClass === 'Opaque') return undefined;
    const commit = ctx.commitSubtreeEdit;
    return () => {
        // Flip the marker in a deep copy of the UNTRANSFORMED node — the
        // subtree channel replaces the destination with this copy, and the
        // qmd writer round-trips the flipped ballot box to `[ ]`/`[x]`.
        const clone = JSON.parse(JSON.stringify(resolved.sourceNode)) as any;
        const items = clone.t === 'OrderedList' ? clone.c?.[1] : clone.c;
        const marker = items?.[itemIndex]?.[0]?.c?.[0];
        if (!isTaskMarker(marker)) {
            // The transformed and source lists disagree (transform reshaped
            // the list) — refuse a blind edit rather than corrupt the doc.
            return;
        }
        marker.c = marker.c === '☐' ? '☒' : '☐';
        commit(JSON.stringify(resolved.sourceEntry), clone);
    };
}

/**
 * The blocks of a task `<li>`: the head block under a `TaskItemContext`
 * provider (so `Plain`/`Para` render the `<label>`), then the rest of the
 * item's blocks outside it (a nested list must not inherit the state).
 */
export function TaskItemBlocks(props: {
    item: BlockNode[];
    checked: boolean;
    onToggle?: () => void;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}) {
    const { item, checked, onToggle, onNavigateToDocument } = props;
    const noop = () => {};
    return (
        <>
            <TaskItemContext.Provider value={{ checked, onToggle }}>
                <Node node={item[0]} onNavigateToDocument={onNavigateToDocument} setLocalAst={noop} />
            </TaskItemContext.Provider>
            {item.slice(1).map((block, j) => (
                <Node key={j} node={block} onNavigateToDocument={onNavigateToDocument} setLocalAst={noop} />
            ))}
        </>
    );
}

/**
 * `<label><input type="checkbox"…/>{children}</label>` — the writer's task
 * markup, rendered by the head block with the marker already stripped.
 * Re-provides `TaskItemContext` as `null` so nothing inside the label
 * (a footnote's blocks, a custom inline) can mistake itself for the head.
 */
export function TaskLabel(props: { state: TaskItemState; children: ReactNode }) {
    const { checked, onToggle } = props.state;
    const noop = () => {};
    // Pointer events must not escape the checkbox: the block-edit surface
    // activates on the host's React onPointerUp (useBlockEditHover), so a
    // toggle click would otherwise ALSO open the item's editor. Text clicks
    // on the label are the opposite case — they should activate the editor,
    // not toggle — so the label suppresses its native input-forwarding for
    // any click that isn't on the input itself.
    const stop = (e: { stopPropagation: () => void }) => e.stopPropagation();
    return (
        <label
            onClick={(e) => {
                if ((e.target as HTMLElement).tagName !== 'INPUT') e.preventDefault();
            }}
        >
            <input
                type="checkbox"
                checked={checked}
                disabled={onToggle === undefined}
                onChange={onToggle ?? noop}
                onClick={stop}
                onPointerDown={stop}
                onPointerUp={stop}
            />
            <TaskItemContext.Provider value={null}>{props.children}</TaskItemContext.Provider>
        </label>
    );
}

/** The current task-item state, or `null` outside a task head block. */
export function useTaskItem(): TaskItemState | null {
    return useContext(TaskItemContext);
}

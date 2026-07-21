import { Node } from '../../framework';
import type { BlockNode } from '../../framework';
import type { PreviewContextValue } from '../PreviewContext';
import type { ResolvedSource } from '../sourceIndex';

/**
 * Task-list support (bd-obkvhlam / bd-tvtknbhx).
 *
 * The reader parses `- [ ] todo` into Pandoc's convention: the item's first
 * inline is `Str "☐"` (unchecked) / `Str "☒"` (checked) followed by `Space`.
 * The native HTML writer renders `<ul class="task-list">` with
 * `<label><input type="checkbox" …/>…</label>` per item; this module gives
 * the q2-preview React renderer the same DOM, plus the interactive half: in
 * an edit-enabled surface, toggling a checkbox flips the ballot-box `Str` in
 * the list's *untransformed* source node and commits it through the subtree
 * edit channel — `apply_node_edit` + the qmd writer's task-marker round-trip
 * turn that into a `[ ]` ↔ `[x]` splice in the source file.
 *
 * Tight (`Plain`-leading) items only: the writer nests the `<label>` inside
 * the loose item's `<p>`, which a wrapper around `<Node node={Para}>` cannot
 * reproduce without invalid HTML (`<p>` is not phrasing content, so it cannot
 * go inside `<label>`). Loose task items keep the default text rendering
 * until Para grows a slot for it.
 */

/** `true`/`false` = task item with that checked state; `null` = not a task item. */
export function taskItemChecked(item: BlockNode[]): boolean | null {
    const head = item[0] as any;
    if (!head || head.t !== 'Plain' || !Array.isArray(head.c)) return null;
    const first = head.c[0];
    if (!first || first.t !== 'Str' || (first.c !== '☐' && first.c !== '☒')) return null;
    if (head.c[1]?.t !== 'Space') return null;
    return first.c === '☒';
}

/** All items are task items (and there is at least one). Drives `class="task-list"`
 * on `<ul>` — bullet lists only, matching Pandoc's HTML writer. */
export function allTaskItems(items: BlockNode[][]): boolean {
    return items.length > 0 && items.every((item) => taskItemChecked(item) !== null);
}

/** The item's head block with the marker `Str` + `Space` stripped, so the
 * remaining inlines render inside the `<label>`. Keeps the pool-id (`s`) so
 * the `<li>` borrow behaves exactly as for a non-task tight item. */
export function strippedTaskHead(head: BlockNode): BlockNode {
    const h = head as any;
    return { ...h, c: h.c.slice(2) };
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
        if (!marker || marker.t !== 'Str' || (marker.c !== '☐' && marker.c !== '☒')) {
            // The transformed and source lists disagree (transform reshaped
            // the list) — refuse a blind edit rather than corrupt the doc.
            return;
        }
        marker.c = marker.c === '☐' ? '☒' : '☐';
        commit(JSON.stringify(resolved.sourceEntry), clone);
    };
}

/** The `<label><input type="checkbox"…/>{stripped head inlines}</label>` body
 * of a task `<li>`. Rest-of-item blocks render after the label (writer parity). */
export function TaskItemBody(props: {
    item: BlockNode[];
    checked: boolean;
    onToggle?: () => void;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}) {
    const { item, checked, onToggle, onNavigateToDocument } = props;
    const noop = () => {};
    // Pointer events must not escape the checkbox: the block-edit surface
    // activates on the host's React onPointerUp (useBlockEditHover), so a
    // toggle click would otherwise ALSO open the item's editor. Text clicks
    // on the label are the opposite case — they should activate the editor,
    // not toggle — so the label suppresses its native input-forwarding for
    // any click that isn't on the input itself.
    const stop = (e: { stopPropagation: () => void }) => e.stopPropagation();
    return (
        <>
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
                <Node
                    node={strippedTaskHead(item[0])}
                    onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={noop}
                />
            </label>
            {item.slice(1).map((block, j) => (
                <Node
                    key={j}
                    node={block}
                    onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={noop}
                />
            ))}
        </>
    );
}

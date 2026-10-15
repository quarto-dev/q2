/**
 * The q2-debug document: a DOM tree that mirrors the AST by node identity,
 * maintained from the edit scripts `reconcileAst` applies. No React.
 *
 * Every AST node owns one box element (`renderNode`), remembered in a
 * WeakMap keyed by the node object. Consecutive Str/Space/SoftBreak
 * inlines are shown as one "Text:" run instead of individual boxes.
 *
 * On each update:
 *  1. `reconcileAst` diffs the fresh parse against the retained AST and
 *     patches the AST in place; untouched node objects survive.
 *  2. Boxes of nodes whose own fields changed are updated in place (label,
 *     text); the element itself is kept.
 *  3. Containers whose child lists changed get their slot contents
 *     re-placed from the AST: existing boxes are moved into position,
 *     new nodes get new boxes, boxes of nodes no longer listed drop out.
 *     Nothing else in the DOM is touched.
 *  4. Highlights are painted from the same report: inserted (green),
 *     changed (yellow), a child unwrapped out (red outline), and a ghost of
 *     each deleted node held in place (red) before it goes. Text runs paint
 *     character-level changes from the script's text facts.
 */

import { reconcileAst, describe, type Elem, type ReconcileResult, type Touched } from '@quarto/preview-renderer/astdiff';
import type { PandocAST } from '@quarto/preview-renderer/framework';
import { NODE_CLASS, refreshNode, renderNode } from './render';
import { emptyLedger, isTextLike, ledgerFrom, segmentsFromLedger, textOf, type Seg, type TextChangeLedger } from './textDiff';

const HIGHLIGHT_MS = 1000;
const FLASH_CLASSES = ['q2-flash-inserted', 'q2-flash-changed', 'q2-flash-removed'];
const GHOST_ATTR = 'data-q2-ghost';
const RUN_ATTR = 'data-q2-run';
/** Marks an element whose text content is AST text (a run's text, a code leaf's text). */
export const TEXT_HOST_ATTR = 'data-q2-text';

/** Solid colour held for the highlight duration. Two identical keyframes on purpose: one would fade in. */
function hold(el: Element, style: Keyframe): Animation | null {
    if (typeof el.animate !== 'function') return null;
    return el.animate([style, style], { duration: HIGHLIGHT_MS, fill: 'forwards' });
}

interface RunRecord {
    el: HTMLElement;
    nodes: unknown[];
    text: string;
}

export class DebugDocument {
    private readonly container: HTMLElement;
    /** Where the document's boxes live; the container itself is left to overlays (caret, status). */
    private readonly content: HTMLElement;
    private retained: PandocAST | null = null;
    private readonly roots = new WeakMap<object, HTMLElement>();
    /** Box element → its node (the inverse of `roots`). */
    private readonly nodeOfBox = new WeakMap<HTMLElement, unknown>();
    private readonly slots = new WeakMap<object, Record<string, HTMLElement>>();
    private readonly elems = new WeakMap<object, Elem>();
    private readonly runKeyOf = new WeakMap<object, object>();
    private readonly runs = new WeakMap<object, RunRecord>();
    /** Text-bearing node → the element holding its text (see `TEXT_HOST_ATTR`). */
    private readonly hostOfNode = new WeakMap<object, HTMLElement>();
    /** Text host → the nodes whose text it holds, in order. */
    private readonly nodesOfHost = new WeakMap<HTMLElement, unknown[]>();
    /** Run host → the inline list (the AST array itself) its nodes live in. */
    private readonly listOfHost = new WeakMap<HTMLElement, unknown[]>();
    /**
     * An inline list with no inlines still shows one empty run, so the
     * caret has somewhere to stop and text can be inserted. Keyed by the
     * list (the applier splices lists in place, so the array is stable).
     */
    private readonly emptyRuns = new WeakMap<unknown[], HTMLElement>();
    private ledger: TextChangeLedger = emptyLedger();
    /** Called after every update with the report, e.g. to migrate a caret. */
    onUpdated: ((result: ReconcileResult) => void) | null = null;

    constructor(container: HTMLElement) {
        this.container = container;
        this.content = document.createElement('div');
        this.content.className = 'q2-debug-content';
        container.append(this.content);
        container.addEventListener('animationend', (e) => {
            if (e.target instanceof Element) e.target.classList.remove(...FLASH_CLASSES);
        });
    }

    /** Forget the retained AST; the next update renders from scratch. */
    reset(): void {
        this.retained = null;
        this.content.replaceChildren();
    }

    /**
     * Reconcile `fresh` in and patch the DOM. Changes are highlighted unless
     * `highlight` is false (the user's own typing needs no announcement).
     */
    update(fresh: PandocAST, { highlight = true } = {}): ReconcileResult {
        const result = reconcileAst(this.retained, fresh);
        this.retained = result.ast;
        if (!result.reused) {
            this.ledger = emptyLedger();
            this.content.replaceChildren();
            this.placeList(this.content, this.retained.blocks, 'block');
        } else {
            this.ledger = highlight ? ledgerFrom(result.touched) : emptyLedger();
            if (highlight) this.ghost(result.touched.removedNodes);
            this.patch(result.touched);
            if (highlight) this.paint(result.touched);
        }
        this.onUpdated?.(result);
        return result;
    }

    get element(): HTMLElement {
        return this.container;
    }

    /** The retained AST (the one the DOM mirrors), or `null` before the first update. */
    get ast(): PandocAST | null {
        return this.retained;
    }

    /** The element holding the document's boxes (no overlays). */
    get contentElement(): HTMLElement {
        return this.content;
    }

    /** The box element of `node`, if rendered. */
    boxOf(node: unknown): HTMLElement | undefined {
        if (node === this.retained) return this.content;
        return node !== null && typeof node === 'object' ? this.roots.get(node) : undefined;
    }

    /** The node whose box `el` is, if `el` is a node's box (runs have none). */
    nodeOf(el: HTMLElement): unknown {
        return this.nodeOfBox.get(el);
    }

    /** The element holding `node`'s text, if `node` is text-bearing and currently on screen. */
    textHostOf(node: unknown): HTMLElement | undefined {
        if (node === null || typeof node !== 'object') return undefined;
        const host = this.hostOfNode.get(node);
        // Entries are never cleared; a node that left the document (deleted
        // with its subtree) still maps to a detached element.
        return host && host.isConnected && !host.closest(`[${GHOST_ATTR}]`) ? host : undefined;
    }

    /** The nodes whose text `host` holds, in order (empty for an empty run). */
    nodesOf(host: HTMLElement): unknown[] {
        return this.nodesOfHost.get(host) ?? [];
    }

    /** The inline list a run host's nodes live in (the AST array itself). */
    listOf(host: HTMLElement): unknown[] | undefined {
        return this.listOfHost.get(host);
    }

    /** All text hosts, in document order. */
    textHosts(): HTMLElement[] {
        return Array.from(this.content.querySelectorAll<HTMLElement>(`[${TEXT_HOST_ATTR}]`)).filter((h) => !h.closest(`[${GHOST_ATTR}]`));
    }

    // --- building and placing -------------------------------------------

    private ensureBox(node: unknown, elem: Elem): HTMLElement {
        const key = node as object;
        const existing = this.roots.get(key);
        if (existing) return existing;
        const { root, slots } = renderNode(node, elem);
        this.roots.set(key, root);
        this.nodeOfBox.set(root, node);
        this.slots.set(key, slots);
        this.elems.set(key, elem);
        this.registerTextHost(node, root);
        this.placeChildren(node);
        return root;
    }

    private registerTextHost(node: unknown, root: HTMLElement): void {
        const host = root.querySelector<HTMLElement>(`:scope > [${TEXT_HOST_ATTR}]`);
        if (!host) return;
        this.hostOfNode.set(node as object, host);
        this.nodesOfHost.set(host, [node]);
    }

    /** Update `node`'s own markup in place and re-place its children. */
    private refreshBox(node: unknown): void {
        const key = node as object;
        const elem = this.elems.get(key);
        const root = this.roots.get(key);
        const slots = this.slots.get(key);
        if (elem === undefined || root === undefined || slots === undefined) return;
        refreshNode(node, elem, { root, slots });
        this.placeChildren(node);
    }

    /** Re-place `node`'s children from the AST into its slot containers. */
    private placeChildren(node: unknown): void {
        if (node === this.retained) {
            this.placeList(this.content, this.retained!.blocks, 'block');
            return;
        }
        const key = node as object;
        const elem = this.elems.get(key);
        const slotEls = this.slots.get(key);
        if (elem === undefined || slotEls === undefined) return;
        for (const slot of describe(node, elem).slots) {
            const el = slotEls[slot.name];
            if (el) this.placeList(el, slot.read(), slot.elem);
        }
    }

    private placeList(el: HTMLElement, children: unknown[], elem: Elem): void {
        const desired: HTMLElement[] = [];
        if (elem === 'inline') {
            const usedKeys = new Set<object>();
            let run: unknown[] = [];
            const flush = () => {
                if (run.length > 0) desired.push(this.runElement(run, usedKeys, children));
                run = [];
            };
            if (children.length === 0) desired.push(this.emptyRun(children));
            for (const child of children) {
                if (isTextLike(child)) {
                    run.push(child);
                } else {
                    flush();
                    desired.push(this.ensureBox(child, elem));
                }
            }
            flush();
        } else {
            for (const child of children) desired.push(this.ensureBox(child, elem));
        }
        syncChildren(el, desired);
    }

    // --- text runs --------------------------------------------------------

    /**
     * A run is keyed by whichever of its nodes was in a run before, so a
     * word added at its start doesn't make it a different run. `used`
     * keeps two runs in one slot from sharing a key.
     */
    private runKey(nodes: unknown[], used: Set<object>): object {
        let key: object | undefined;
        for (const n of nodes) {
            const k = this.runKeyOf.get(n as object);
            if (k !== undefined && !used.has(k)) { key = k; break; }
        }
        if (key === undefined || used.has(key)) key = nodes[0] as object;
        used.add(key);
        for (const n of nodes) this.runKeyOf.set(n as object, key);
        return key;
    }

    private runElement(nodes: unknown[], used: Set<object>, list: unknown[]): HTMLElement {
        const key = this.runKey(nodes, used);
        const text = nodes.map(textOf).join('');
        const rec = this.runs.get(key);
        if (rec && rec.text === text && rec.nodes.length === nodes.length && rec.nodes.every((n, i) => n === nodes[i])) {
            this.listOfHost.set(this.nodesHost(rec.el), list);
            return rec.el;
        }
        const el = buildRun(segmentsFromLedger(nodes, this.ledger), text);
        this.runs.set(key, { el, nodes: nodes.slice(), text });
        const host = this.nodesHost(el);
        for (const n of nodes) this.hostOfNode.set(n as object, host);
        this.nodesOfHost.set(host, nodes.slice());
        this.listOfHost.set(host, list);
        return el;
    }

    /**
     * The run shown for an empty inline list. The list itself stands in as
     * the caret's node there (`hostOfNode` maps it to the empty host).
     */
    private emptyRun(list: unknown[]): HTMLElement {
        let el = this.emptyRuns.get(list);
        if (!el) {
            el = buildRun([], '');
            el.classList.add('empty');
            // Not a text run: say so, rather than showing an empty "Text:".
            el.querySelector(':scope > .label')!.textContent = '(no inlines)';
            this.emptyRuns.set(list, el);
        }
        const host = this.nodesHost(el);
        this.hostOfNode.set(list, host);
        this.nodesOfHost.set(host, []);
        this.listOfHost.set(host, list);
        return el;
    }

    private nodesHost(run: HTMLElement): HTMLElement {
        return run.querySelector<HTMLElement>(`[${TEXT_HOST_ATTR}]`)!;
    }

    // --- highlighting -----------------------------------------------------

    /** Leave a static copy of each deleted node's box in place, held red, then drop it. */
    private ghost(nodes: unknown[]): void {
        for (const node of nodes) {
            const box = this.roots.get(node as object);
            if (!box || !box.isConnected) continue;
            const ghost = box.cloneNode(true) as HTMLElement;
            ghost.setAttribute(GHOST_ATTR, '');
            ghost.setAttribute('aria-hidden', 'true');
            ghost.style.pointerEvents = 'none';
            ghost.querySelectorAll(`[${GHOST_ATTR}]`).forEach((g) => g.remove());
            box.before(ghost);
            const done = () => ghost.remove();
            const anim = hold(ghost, { backgroundColor: '#f29b9b', boxShadow: '0 0 0 2px #e03131' });
            if (anim) anim.finished.then(done, done);
            window.setTimeout(done, HIGHLIGHT_MS * 2);
        }
    }

    private patch(touched: Touched): void {
        for (const node of touched.changedNodes) this.refreshBox(node);
        for (const node of touched.relisted) this.placeChildren(node);
    }

    private paint(touched: Touched): void {
        const flash = (el: Element, cls: string) => {
            if (el.classList.contains(cls)) {
                el.classList.remove(cls);
                void (el as HTMLElement).offsetWidth;
            }
            el.classList.add(cls);
        };
        for (const node of touched.insertedSubtrees) {
            const box = this.boxOf(node);
            if (!box) continue;
            flash(box, 'q2-flash-inserted');
            for (const inner of box.querySelectorAll(`.${NODE_CLASS}`)) flash(inner, 'q2-flash-inserted');
        }
        for (const node of touched.createdNodes) {
            const box = this.boxOf(node);
            if (box) flash(box, 'q2-flash-inserted');
        }
        for (const node of touched.changedNodes) {
            const box = this.boxOf(node);
            if (box) flash(box, 'q2-flash-changed');
        }
        for (const node of touched.removedFrom) {
            const box = this.boxOf(node);
            if (box) flash(box, 'q2-flash-removed');
        }
    }
}

/**
 * Make `el`'s element children equal `desired`, in order, moving existing
 * elements rather than recreating them. Ghosts (`data-q2-ghost`) are left
 * where they are and never counted.
 */
function syncChildren(el: HTMLElement, desired: HTMLElement[]): void {
    const isGhost = (n: Element | null) => n !== null && n.hasAttribute(GHOST_ATTR);
    const nextReal = (n: Element | null): Element | null => {
        while (isGhost(n)) n = n!.nextElementSibling;
        return n;
    };
    let cursor = nextReal(el.firstElementChild);
    for (const want of desired) {
        if (cursor === want) {
            cursor = nextReal(cursor.nextElementSibling);
        } else {
            el.insertBefore(want, cursor);
        }
    }
    while (cursor !== null) {
        const next = nextReal(cursor.nextElementSibling);
        cursor.remove();
        cursor = next;
    }
}

function buildRun(segs: Seg[], text: string): HTMLElement {
    const el = document.createElement('span');
    el.className = `${NODE_CLASS} inline run`;
    el.setAttribute(RUN_ATTR, '');
    const label = document.createElement('strong');
    label.className = 'label';
    label.textContent = 'Text: ';
    const host = document.createElement('span');
    host.className = 'run-text';
    host.setAttribute(TEXT_HOST_ATTR, '');
    el.append(label, host);
    let marked = false;
    for (const seg of segs) {
        if (seg.kind === 'eq') {
            host.append(seg.text);
            continue;
        }
        marked = true;
        const span = document.createElement('span');
        span.className = seg.kind === 'ins' ? 'q2-ins' : 'q2-del';
        span.textContent = seg.text;
        host.append(span);
        hold(span, { backgroundColor: seg.kind === 'ins' ? '#8ee6a0' : '#f29b9b' });
    }
    if (marked) {
        // Collapse to plain text when the highlight ends: removed characters
        // go, inserted ones lose their span.
        window.setTimeout(() => host.replaceChildren(text), HIGHLIGHT_MS);
    }
    return el;
}

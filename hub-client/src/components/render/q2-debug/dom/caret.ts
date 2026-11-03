/**
 * An AST caret for q2-debug.
 *
 * The caret is a position *in the AST*: a text-bearing node (a `Str`,
 * `Space` or `SoftBreak` inline, or a text leaf such as a `CodeBlock`) and
 * a character offset into that node's text. It is drawn as our own
 * element over the node's text host; the document is never editable and
 * the browser selection is never used.
 *
 * Because the document patches its AST in place, the caret's node object
 * survives edits elsewhere unchanged. After each update the caret is
 * migrated using the applier's report: a text change maps the offset
 * through the character diff (an editor cursor, not a fixed index), a
 * `Str` split moves it into the second half when it sat past the split
 * point, a merge moves it into the absorbing node, and a deleted node
 * hands the caret to the neighbour the deletion was recorded against.
 *
 * Interaction: click to place (hit-tested with `caretPositionFromPoint`,
 * falling back to the nearest text host), ArrowLeft / ArrowRight to move
 * by one character, across run and block boundaries in document order.
 *
 * Selection: a second AST position, the anchor, with the caret as head.
 * Drag to select, Shift+click or Shift+Arrow to extend, plain Arrow to
 * collapse to an edge. Both ends migrate through edits like the caret.
 * The selection is painted as one hull per visual row in an SVG layer
 * (joining the rects of the selected text with diagonals so a Space box
 * next to a taller run reads as one highlight), blended with the text.
 *
 * Editing: a printable key replaces the selection (or inserts at the
 * caret) and Backspace deletes the selection (or the character before the
 * caret). The edit is made on a copy of the AST (dom/edit.ts), reconciled
 * into the document right away — so the caret migrates like it does for
 * any other update and the next key sees the new text — and handed to
 * `onEdit` for the host to write back as QMD.
 */

import type { ReconcileResult, Touched } from '@quarto/preview-renderer/astdiff';
import type { PandocAST } from '@quarto/preview-renderer/framework';
import type { DebugDocument } from './document';
import { replaceText, type HostPosition } from './edit';
import { isTextLike, mapOffsetThroughChange, textOf } from './textDiff';

export interface CaretPosition {
    node: unknown;
    /** Offset into `textOf(node)`, in UTF-16 units, `0..length`. */
    offset: number;
}

const DEL_CLASS = 'q2-del';

export class AstCaret {
    private readonly doc: DebugDocument;
    private readonly container: HTMLElement;
    /** Positioned wrapper: holds the blinking bar and the name flag. */
    private readonly el: HTMLDivElement;
    private readonly bar: HTMLDivElement;
    /** Name flag riding on top of the caret, in the user's colour. */
    private readonly flag: HTMLDivElement;
    /** Selection hulls, painted under the caret. */
    private readonly layer: SVGSVGElement;
    private position: CaretPosition | null = null;
    /** Other end of the selection; `null` when collapsed. */
    private anchor: CaretPosition | null = null;
    private color = '#1a73e8';
    private dragging = false;
    /** Updates the caret has lived through since it was last placed (debugging). */
    updatesSurvived = 0;
    private placing = false;
    /** Why the caret is where it is (exposed via `current` for debugging). */
    reason = '';
    /** Receives each edited AST (already applied to the document) for the host to write back. */
    onEdit: ((ast: PandocAST) => void) | null = null;
    /**
     * Where the caret goes after the edit in flight: an offset into the host
     * it migrates to; `list` is the inline list the edit started in, the
     * fallback when every node around the caret was deleted.
     */
    private landing: { offset: number; list: unknown[] | undefined } | null = null;

    constructor(doc: DebugDocument) {
        this.doc = doc;
        this.container = doc.element;
        this.el = document.createElement('div');
        this.el.className = 'q2-caret hidden';
        this.bar = document.createElement('div');
        this.bar.className = 'q2-caret-bar';
        this.flag = document.createElement('div');
        this.flag.className = 'q2-caret-flag hidden';
        this.el.append(this.bar, this.flag);
        this.layer = document.createElementNS(SVG_NS, 'svg');
        this.layer.setAttribute('class', 'q2-selection');
        this.container.append(this.layer, this.el);
        // The click's default action (which would move focus into the iframe)
        // is suppressed below, so take focus explicitly; without it the
        // parent's editor keeps the keyboard.
        this.container.tabIndex = -1;

        this.container.addEventListener('pointerdown', (e) => this.onPointerDown(e));
        this.container.addEventListener('pointermove', (e) => this.onPointerMove(e));
        this.container.addEventListener('pointerup', (e) => this.endDrag(e));
        this.container.addEventListener('pointercancel', (e) => this.endDrag(e));
        window.addEventListener('keydown', (e) => this.onKeyDown(e));
        window.addEventListener('resize', () => this.schedulePlace());
        window.addEventListener('scroll', () => this.schedulePlace(), true);
        // Any DOM change in the document (collapsing run highlights, ghosts
        // going away, structural patches) can move text; re-place on the next
        // frame. Watch the content element only: the caret lives beside it in
        // the container, and observing our own writes would loop.
        new MutationObserver(() => this.schedulePlace()).observe(doc.contentElement, { childList: true, subtree: true, characterData: true });
        doc.onUpdated = (result) => this.afterUpdate(result);
    }

    get current(): CaretPosition | null {
        return this.position;
    }

    /** The selection in document order, or `null` when collapsed. */
    get selection(): { start: CaretPosition; end: CaretPosition } | null {
        if (!this.position || !this.anchor) return null;
        const cmp = this.compare(this.anchor, this.position);
        if (cmp === 0) return null;
        return cmp < 0 ? { start: this.anchor, end: this.position } : { start: this.position, end: this.anchor };
    }

    /** Select from `anchor` to `head` (the caret sits at `head`). */
    select(anchor: CaretPosition, head: CaretPosition, reason = ''): void {
        this.anchor = this.clamp(anchor);
        this.set(head, reason, true);
    }

    private clamp(pos: CaretPosition): CaretPosition {
        const len = textOf(pos.node).length;
        return { node: pos.node, offset: Math.max(0, Math.min(pos.offset, len)) };
    }

    /** Document order of two positions: negative when `a` comes first. */
    private compare(a: CaretPosition, b: CaretPosition): number {
        const ha = this.toHost(a);
        const hb = this.toHost(b);
        if (!ha || !hb) return 0;
        if (ha.host !== hb.host) {
            const hosts = this.doc.textHosts();
            return hosts.indexOf(ha.host) - hosts.indexOf(hb.host);
        }
        return ha.offset - hb.offset;
    }

    /** Name and colour shown on the caret's flag; `null` hides the flag. */
    setUser(user: { name: string; color: string } | null): void {
        this.flag.textContent = user?.name ?? '';
        this.flag.classList.toggle('hidden', user === null || user.name === '');
        this.color = user?.color || '#1a73e8';
        this.bar.style.backgroundColor = this.color;
        this.flag.style.backgroundColor = this.color;
        this.render();
    }

    /**
     * Put the caret at `(node, offset)`; offset is clamped to the node's
     * text. Collapses the selection unless `keepAnchor`.
     */
    set(position: CaretPosition | null, reason = '', keepAnchor = false): void {
        this.position = position === null ? null : this.clamp(position);
        if (!keepAnchor || position === null) this.anchor = null;
        this.updatesSurvived = 0;
        this.reason = reason;
        this.render();
    }

    // --- model ↔ hosts ----------------------------------------------------------

    private toHost(pos: CaretPosition): HostPosition | null {
        const host = this.doc.textHostOf(pos.node);
        if (!host) return null;
        // An empty run: its list is the node, and the only offset is 0.
        if (this.doc.nodesOf(host).length === 0) return { host, offset: 0 };
        let offset = 0;
        for (const n of this.doc.nodesOf(host)) {
            if (n === pos.node) return { host, offset: offset + pos.offset };
            offset += textOf(n).length;
        }
        return null;
    }

    /** Boundaries between two nodes attach to the *following* node. */
    private fromHost(hp: HostPosition): CaretPosition | null {
        const nodes = this.doc.nodesOf(hp.host);
        if (nodes.length === 0) {
            const list = this.doc.listOf(hp.host);
            return list ? { node: list, offset: 0 } : null;
        }
        let offset = hp.offset;
        for (let i = 0; i < nodes.length; i++) {
            const len = textOf(nodes[i]).length;
            if (offset < len || i === nodes.length - 1) return { node: nodes[i], offset: Math.min(offset, len) };
            offset -= len;
        }
        return null;
    }

    private hostText(host: HTMLElement): number {
        return this.doc.nodesOf(host).reduce((n: number, node) => n + textOf(node).length, 0);
    }

    // --- DOM text ↔ host offsets -------------------------------------------------

    /** Text nodes of a host that carry AST text (removed-character spans excluded). */
    private textNodes(host: HTMLElement): Text[] {
        const out: Text[] = [];
        const walker = document.createTreeWalker(host, NodeFilter.SHOW_TEXT);
        for (let n = walker.nextNode(); n; n = walker.nextNode()) {
            const t = n as Text;
            if (t.parentElement?.closest(`.${DEL_CLASS}`)) continue;
            out.push(t);
        }
        return out;
    }

    private hostOffsetOf(host: HTMLElement, textNode: Node, offset: number): number {
        let total = 0;
        for (const t of this.textNodes(host)) {
            if (t === textNode) return total + Math.min(offset, t.length);
            total += t.length;
        }
        return total;
    }

    private domAt(host: HTMLElement, offset: number): { node: Text; offset: number } | null {
        let remaining = offset;
        const nodes = this.textNodes(host);
        for (let i = 0; i < nodes.length; i++) {
            const t = nodes[i];
            if (remaining <= t.length && (remaining < t.length || i === nodes.length - 1)) return { node: t, offset: remaining };
            remaining -= t.length;
        }
        return null;
    }

    // --- drawing -----------------------------------------------------------------

    private schedulePlace(): void {
        if (this.placing) return;
        this.placing = true;
        requestAnimationFrame(() => {
            this.placing = false;
            this.render();
        });
    }

    private render(): void {
        this.paintSelection();
        const pos = this.position;
        const hp = pos ? this.toHost(pos) : null;
        if (!pos || !hp) {
            this.el.classList.add('hidden');
            return;
        }
        let rect: DOMRect | null = null;
        const at = this.domAt(hp.host, hp.offset);
        if (at) {
            const range = document.createRange();
            range.setStart(at.node, at.offset);
            range.collapse(true);
            const rects = range.getClientRects();
            rect = rects.length > 0 ? rects[0] : range.getBoundingClientRect();
        }
        if (!rect || (rect.width === 0 && rect.height === 0)) {
            // Empty hosts and some engines' collapsed ranges report no rect.
            const box = hp.host.getBoundingClientRect();
            rect = new DOMRect(hp.offset === 0 ? box.left : box.right, box.top, 0, box.height);
        }
        const base = this.container.getBoundingClientRect();
        this.el.style.left = `${rect.left - base.left + this.container.scrollLeft - 1}px`;
        this.el.style.top = `${rect.top - base.top + this.container.scrollTop}px`;
        this.el.style.height = `${Math.max(rect.height, 12)}px`;
        this.el.classList.remove('hidden');
        // Restart the blink so the caret is visible right after a move.
        this.bar.style.animation = 'none';
        void this.bar.offsetWidth;
        this.bar.style.animation = '';
    }

    // --- input -------------------------------------------------------------------

    private onPointerDown(e: PointerEvent): void {
        if (e.button !== 0) return;
        const pos = this.positionAtPoint(e.clientX, e.clientY);
        if (!pos) return;
        e.preventDefault();
        this.container.focus({ preventScroll: true });
        if (e.shiftKey && this.position) {
            this.select(this.anchor ?? this.position, pos, 'shift-click');
        } else {
            this.set(pos, 'click');
            this.anchor = pos;
        }
        this.dragging = true;
        this.container.setPointerCapture(e.pointerId);
    }

    private onPointerMove(e: PointerEvent): void {
        if (!this.dragging || (e.buttons & 1) === 0 || !this.anchor) return;
        const head = this.positionAtPoint(e.clientX, e.clientY);
        if (!head) return;
        if (this.position && head.node === this.position.node && head.offset === this.position.offset) return;
        this.select(this.anchor, head, 'drag');
    }

    private endDrag(e: PointerEvent): void {
        if (!this.dragging) return;
        this.dragging = false;
        if (this.container.hasPointerCapture(e.pointerId)) this.container.releasePointerCapture(e.pointerId);
        // A click without movement leaves the anchor equal to the head: collapsed.
        if (this.anchor && this.position && this.compare(this.anchor, this.position) === 0) this.anchor = null;
    }

    private onKeyDown(e: KeyboardEvent): void {
        if (!this.position) return;
        if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
            e.preventDefault();
            this.move(e.key === 'ArrowLeft' ? -1 : 1, e.shiftKey);
            return;
        }
        if (e.ctrlKey || e.metaKey || e.altKey || e.isComposing) return;
        if (e.key === 'Backspace') {
            e.preventDefault();
            this.replace('', true);
        } else if (e.key.length === 1) {
            e.preventDefault();
            this.replace(e.key, false);
        }
    }

    /**
     * Move one character; at a host's edge, continue in the next / previous
     * host. With `extend` the anchor stays and the selection grows; without
     * it a selection collapses to its edge in that direction.
     */
    move(dir: -1 | 1, extend = false): void {
        if (!this.position) return;
        const sel = this.selection;
        if (sel && !extend) {
            this.set(dir < 0 ? sel.start : sel.end, dir === 1 ? 'right' : 'left');
            return;
        }
        if (extend && !this.anchor) this.anchor = this.position;
        const hp = this.toHost(this.position);
        const next = hp ? this.step(hp, dir) : null;
        const pos = next ? this.fromHost(next) : null;
        if (pos) this.set(pos, dir === 1 ? 'right' : 'left', extend);
    }

    /** The host position one character from `hp`; at a host's edge, the next / previous host's edge. */
    private step(hp: HostPosition, dir: -1 | 1): HostPosition | null {
        const len = this.hostText(hp.host);
        if (dir === 1 && hp.offset < len) return { host: hp.host, offset: hp.offset + 1 };
        if (dir === -1 && hp.offset > 0) return { host: hp.host, offset: hp.offset - 1 };
        const hosts = this.doc.textHosts();
        const i = hosts.indexOf(hp.host);
        if (i === -1) return null;
        const j = i + dir;
        if (j < 0 || j >= hosts.length) return null;
        return { host: hosts[j], offset: dir === 1 ? 0 : this.hostText(hosts[j]) };
    }

    // --- editing -------------------------------------------------------------------

    /**
     * Replace the selection with `text`; with no selection, insert at the
     * caret, or with `backwards` replace the character before it. The
     * caret collapses to the range's start and, once the edit has been
     * reconciled in, lands right after the inserted text.
     */
    private replace(text: string, backwards: boolean): void {
        if (!this.position || !this.onEdit) return;
        const sel = this.selection;
        const end = this.toHost(sel ? sel.end : this.position);
        if (!end) return;
        const start = sel ? this.toHost(sel.start) : backwards ? this.step(end, -1) : end;
        if (!start) return;
        if (text === '' && this.charsBetween(start, end) === 0) return;
        const ast = replaceText(this.doc, start, end, text);
        if (!ast) return;
        this.position = sel ? sel.start : this.position;
        this.anchor = null;
        this.landing = { offset: start.offset + text.length, list: this.doc.listOf(start.host) };
        this.doc.update(ast, { highlight: false });
        this.onEdit(ast);
    }

    /** Number of characters in `[start, end)` across the hosts it spans. */
    private charsBetween(start: HostPosition, end: HostPosition): number {
        const hosts = this.doc.textHosts();
        const i = hosts.indexOf(start.host);
        const j = hosts.indexOf(end.host);
        if (i === -1 || j === -1 || j < i) return 0;
        let n = 0;
        for (let k = i; k <= j; k++) {
            const a = k === i ? start.offset : 0;
            const b = k === j ? end.offset : this.hostText(hosts[k]);
            n += Math.max(0, b - a);
        }
        return n;
    }

    // --- selection painting ------------------------------------------------------

    private paintSelection(): void {
        this.layer.replaceChildren();
        const sel = this.selection;
        if (!sel) return;
        const start = this.toHost(sel.start);
        const end = this.toHost(sel.end);
        if (!start || !end) return;
        const hosts = this.doc.textHosts();
        const from = hosts.indexOf(start.host);
        const to = hosts.indexOf(end.host);
        if (from === -1 || to === -1) return;
        // Size the layer to the scrollable content so hulls aren't clipped.
        this.layer.setAttribute('width', String(this.container.scrollWidth));
        this.layer.setAttribute('height', String(this.container.scrollHeight));
        const base = this.container.getBoundingClientRect();
        const boxes: Box[] = [];
        let seq = 0;
        for (let i = from; i <= to; i++) {
            const host = hosts[i];
            const a = i === from ? start.offset : 0;
            const b = i === to ? end.offset : this.hostText(host);
            if (b <= a) continue;
            const da = this.domAt(host, a);
            const db = this.domAt(host, b);
            if (!da || !db) continue;
            const range = document.createRange();
            range.setStart(da.node, da.offset);
            range.setEnd(db.node, db.offset);
            for (const r of range.getClientRects()) {
                if (r.width <= 0 && r.height <= 0) continue;
                const left = r.left - base.left + this.container.scrollLeft;
                const top = r.top - base.top + this.container.scrollTop;
                boxes.push({ left, top, right: left + Math.max(r.width, 1), bottom: top + r.height, start: i * 1e6 + a, seq: seq++ });
            }
        }
        for (const d of hullPaths(boxes)) {
            const path = document.createElementNS(SVG_NS, 'path');
            path.setAttribute('class', 'q2-selection-hull');
            path.setAttribute('fill', tint(this.color));
            path.setAttribute('d', d);
            this.layer.append(path);
        }
    }

    /** Resolve a viewport point to an AST position: hit-test, else nearest host edge. */
    positionAtPoint(x: number, y: number): CaretPosition | null {
        const hit = caretHitFromPoint(x, y);
        if (hit) {
            const host = (hit.node instanceof Element ? hit.node : hit.node.parentElement)?.closest<HTMLElement>('[data-q2-text]');
            if (host) {
                const offset = hit.node.nodeType === Node.TEXT_NODE ? this.hostOffsetOf(host, hit.node, hit.offset) : (x < midX(host) ? 0 : this.hostText(host));
                return this.fromHost({ host, offset });
            }
        }
        // Not over text: nearest host by vertical distance, then horizontal; land on its nearer edge.
        let best: HTMLElement | null = null;
        let bestDy = Infinity;
        let bestDx = Infinity;
        for (const host of this.doc.textHosts()) {
            const r = host.getBoundingClientRect();
            const dy = y < r.top ? r.top - y : y > r.bottom ? y - r.bottom : 0;
            const dx = x < r.left ? r.left - x : x > r.right ? x - r.right : 0;
            if (dy < bestDy || (dy === bestDy && dx < bestDx)) { best = host; bestDy = dy; bestDx = dx; }
        }
        if (!best) return null;
        return this.fromHost({ host: best, offset: x < midX(best) ? 0 : this.hostText(best) });
    }

    // --- surviving edits -----------------------------------------------------------

    private afterUpdate(result: ReconcileResult): void {
        const landing = this.landing;
        this.landing = null;
        if (!this.position) return;
        if (!result.reused) {
            // The document was rebuilt from a fresh parse; the nodes are gone.
            this.set(null, 'document reloaded');
            return;
        }
        const t = result.touched;
        let migrated = this.migrate(this.position, t);
        const anchor = this.anchor ? this.migrate(this.anchor, t) : null;
        if (migrated === null && anchor === null && landing?.list && this.doc.textHostOf(landing.list)) {
            // Our edit deleted everything around the caret; the list it was
            // in now shows as an empty run, which is where the caret belongs.
            migrated = { node: landing.list, offset: 0 };
        }
        if (migrated === null && anchor === null) {
            this.position = null;
            this.anchor = null;
            this.reason = 'node deleted';
            this.render();
            return;
        }
        // One end gone: collapse onto the other.
        let head = migrated ?? anchor!;
        if (landing !== null) {
            // Our own edit: the text before the caret is unchanged in the host
            // it migrated to, so the landing offset counts from that host's start.
            const hp = this.toHost(head);
            const pos = hp ? this.fromHost({ host: hp.host, offset: landing.offset }) : null;
            if (pos) head = pos;
        }
        const moved = head.node !== this.position.node || head.offset !== this.position.offset;
        this.position = head;
        this.anchor = migrated === null || landing !== null ? null : anchor;
        if (this.anchor && this.compare(this.anchor, this.position) === 0) this.anchor = null;
        this.updatesSurvived++;
        this.reason = landing !== null ? 'typed' : moved ? 'followed edit' : '';
        this.render();
    }

    private migrate(pos: CaretPosition, t: Touched): CaretPosition | null {
        let { node, offset } = pos;
        if (Array.isArray(node)) {
            // On an empty run (the list is the node): follow text inserted
            // into the list, else stay while the list is still shown empty.
            if (node.length > 0) return isTextLike(node[0]) ? { node: node[0], offset: 0 } : null;
            return this.doc.textHostOf(node) ? { node, offset: 0 } : null;
        }
        // A text cursor, not a fixed index: follow the character diff.
        for (const c of t.textChanges) {
            if (c.node === node) offset = mapOffsetThroughChange(c.old, c.new, offset);
        }
        for (const s of t.textSplits) {
            if (s.node === node && offset > s.at) { node = s.second; offset -= s.at; }
        }
        for (const m of t.textMerges) {
            if (m.absorbed === node) { node = m.node; offset += m.at; }
        }
        const removed = t.removedText.find((r) => r.node === node);
        if (removed) {
            // Hand the caret to a surviving neighbour, resolving through
            // other removals recorded in the same script.
            const byNode = new Map(t.removedText.map((r) => [r.node, r]));
            const resolve = (n: unknown | null, side: 'before' | 'after'): unknown | null => {
                let cur = n;
                for (let g = 0; cur !== null && byNode.has(cur) && g < 1000; g++) cur = byNode.get(cur)![side];
                return cur;
            };
            const after = resolve(removed.after, 'after');
            const before = resolve(removed.before, 'before');
            if (after !== null) return { node: after, offset: textOf(after).length };
            if (before !== null) return { node: before, offset: 0 };
            return null;
        }
        if (!this.doc.textHostOf(node)) {
            // Inside a deleted subtree (or otherwise no longer rendered).
            return null;
        }
        return { node, offset: Math.min(offset, textOf(node).length) };
    }
}

const SVG_NS = 'http://www.w3.org/2000/svg';

/** Rect in the container's scroll space. `start` orders boxes by document position. */
interface Box {
    left: number; top: number; right: number; bottom: number;
    start: number;
    seq: number;
}

/**
 * Group boxes into rows following document order (a new row starts when a
 * box goes back left of where the previous one ended, like wrapped text)
 * and build one hull polygon per row: along the top, each box's top edge
 * then a diagonal to the next box's top-left; back along the bottom the
 * same way. Boxes of different heights thus read as one highlight.
 */
function hullPaths(boxes: Box[]): string[] {
    if (boxes.length === 0) return [];
    const sorted = [...boxes].sort((a, b) => a.start - b.start || a.seq - b.seq);
    const rows: Box[][] = [];
    let prev: Box | null = null;
    for (const b of sorted) {
        const wraps = prev !== null && b.left < prev.right - 1;
        if (!prev || wraps) rows.push([b]);
        else rows[rows.length - 1].push(b);
        prev = b;
    }
    const f = (n: number) => n.toFixed(1);
    return rows.map((row) => {
        row.sort((a, b) => a.left - b.left);
        let d = `M${f(row[0].left)} ${f(row[0].top)}`;
        for (let i = 0; i < row.length; i++) {
            const r = row[i];
            d += ` L${f(r.right)} ${f(r.top)}`;
            if (i + 1 < row.length) d += ` L${f(row[i + 1].left)} ${f(row[i + 1].top)}`;
        }
        for (let i = row.length - 1; i >= 0; i--) {
            const r = row[i];
            d += ` L${f(r.right)} ${f(r.bottom)}`;
            d += ` L${f(r.left)} ${f(r.bottom)}`;
            if (i > 0) d += ` L${f(row[i - 1].right)} ${f(row[i - 1].bottom)}`;
        }
        return d + ' Z';
    });
}

/**
 * A light, solid tint of `hex` (mixed 3:1 with white). The selection layer
 * sits over the text and uses `mix-blend-mode: multiply`, so a solid tint
 * reads as a highlight with the text showing through — no alpha needed.
 */
function tint(hex: string): string {
    const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
    if (!m) return '#cfe0ff';
    const n = parseInt(m[1], 16);
    const mix = (c: number) => Math.round(c * 0.3 + 255 * 0.7).toString(16).padStart(2, '0');
    return `#${mix((n >> 16) & 255)}${mix((n >> 8) & 255)}${mix(n & 255)}`;
}

function midX(el: Element): number {
    const r = el.getBoundingClientRect();
    return r.left + r.width / 2;
}

function caretHitFromPoint(x: number, y: number): { node: Node; offset: number } | null {
    const doc = document as Document & {
        caretPositionFromPoint?: (x: number, y: number) => { offsetNode: Node; offset: number } | null;
        caretRangeFromPoint?: (x: number, y: number) => Range | null;
    };
    if (doc.caretPositionFromPoint) {
        const pos = doc.caretPositionFromPoint(x, y);
        return pos ? { node: pos.offsetNode, offset: pos.offset } : null;
    }
    if (doc.caretRangeFromPoint) {
        const range = doc.caretRangeFromPoint(x, y);
        return range ? { node: range.startContainer, offset: range.startOffset } : null;
    }
    return null;
}

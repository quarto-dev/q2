/**
 * Markup for one AST node, generically from its `describe()` shape: a
 * labelled box with the node's kind, properties, attr and text, and one
 * container element per child slot. Children are *not* rendered here —
 * `DebugDocument` places child boxes into the slot containers, so that a
 * node's own markup can be rebuilt without touching its children's DOM.
 */

import { describe, type Desc, type Elem } from '@quarto/preview-renderer/astdiff';
import type { Attr } from '@quarto/preview-renderer/framework';

export interface Rendered {
    root: HTMLElement;
    slots: Record<string, HTMLElement>;
}

export const NODE_CLASS = 'node';

const PROP_LABEL_MAX = 40;

function short(s: string): string {
    return s.length > PROP_LABEL_MAX ? s.slice(0, PROP_LABEL_MAX - 1) + '…' : s;
}

function attrSummary(attr: Attr): string {
    const parts: string[] = [];
    if (attr[0]) parts.push(`#${attr[0]}`);
    for (const c of attr[1]) parts.push(`.${c}`);
    for (const [k, v] of attr[2]) parts.push(`${k}=${JSON.stringify(v)}`);
    return parts.join(' ');
}

export function labelFor(desc: Desc): string {
    const props: string[] = [];
    for (const [name, access] of Object.entries(desc.props)) {
        const v = access.get();
        if (v === undefined || v === null) continue;
        props.push(`${name}=${short(JSON.stringify(v))}`);
    }
    let label = desc.kind;
    if (props.length > 0) label += `(${props.join(', ')})`;
    const attr = desc.attr ? (desc.attr.get() as Attr | undefined) : undefined;
    if (attr && (attr[0] || attr[1].length > 0 || attr[2].length > 0)) label += ` ${attrSummary(attr)}`;
    return label;
}

export function isInlineElem(elem: Elem): boolean {
    return elem === 'inline' || elem === 'citation';
}

function labelText(desc: Desc): string {
    return labelFor(desc) + (desc.text || desc.slots.length > 0 ? ':' : '');
}

/**
 * Bring an existing box's own markup (label, text) up to date with the
 * node, in place. A node's kind — and so its slot set — never changes
 * for one node identity (a type change is a new node), so the element and
 * its slot containers are kept.
 */
export function refreshNode(node: unknown, elem: Elem, rendered: Rendered): void {
    const desc = describe(node, elem);
    const label = rendered.root.querySelector(':scope > .label');
    const text = labelText(desc);
    if (label && label.textContent !== text) label.textContent = text;
    if (desc.text) {
        const code = rendered.root.querySelector(':scope > code.text');
        const value = String(desc.text.get());
        if (code && code.textContent !== value) code.textContent = value;
    }
}

export function renderNode(node: unknown, elem: Elem): Rendered {
    const desc = describe(node, elem);
    const inline = isInlineElem(elem);
    const root = document.createElement(inline ? 'span' : 'div');
    root.className = `${NODE_CLASS} ${inline ? 'inline' : 'block'}`;
    root.dataset.kind = desc.kind;

    const label = document.createElement('strong');
    label.className = 'label';
    label.textContent = labelText(desc);
    root.append(label);

    if (desc.text) {
        const code = document.createElement('code');
        code.className = 'text';
        code.setAttribute('data-q2-text', '');
        code.textContent = String(desc.text.get());
        root.append(' ', code);
    }

    const slots: Record<string, HTMLElement> = {};
    for (const slot of desc.slots) {
        if (desc.slots.length > 1) {
            const caption = document.createElement('em');
            caption.className = 'slot-label';
            caption.textContent = ` ${slot.name}:`;
            root.append(caption);
        }
        const el = document.createElement(isInlineElem(slot.elem) ? 'span' : 'div');
        el.className = 'slot';
        el.dataset.slot = slot.name;
        root.append(el);
        slots[slot.name] = el;
    }
    return { root, slots };
}

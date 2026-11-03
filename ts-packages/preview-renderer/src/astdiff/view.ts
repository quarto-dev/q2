/**
 * A uniform, annotation-free view of Pandoc-JSON nodes.
 *
 * Every node kind is described as: a kind name, an optional `Attr`, a set of
 * scalar properties, optional text, and named child slots. The description
 * carries live accessors into the underlying JSON value, so the same table
 * drives both the differ (reads) and the applier (in-place writes).
 *
 * Annotations stamped by the Rust JSON writer (`s`, `a`, `l`, `targetS`,
 * `textS`, `captionS`, ...) are never part of the view; see `ANNOTATION_KEYS`.
 */

import type { Attr } from '../framework/types';

export type Elem =
    | 'block'
    | 'inline'
    | 'list_item'
    | 'line'
    | 'definition_item'
    | 'definition'
    | 'citation'
    | 'table_head'
    | 'table_body'
    | 'table_foot'
    | 'row'
    | 'cell';

export interface SlotAccess {
    name: string;
    elem: Elem;
    /** The child list (an empty array when the slot is absent/null). */
    read(): unknown[];
    /** The live child list, created in place if absent. Mutate via splice. */
    write(): unknown[];
    /**
     * Point the slot at a different array. Absent for kinds whose value *is*
     * the child list (list items, lines, definitions).
     */
    replace?(next: unknown[]): void;
}

export interface PropAccess {
    get(): unknown;
    set(v: unknown): void;
}

export interface Desc {
    kind: string;
    elem: Elem;
    attr: PropAccess | null;
    props: Record<string, PropAccess>;
    text: PropAccess | null;
    slots: SlotAccess[];
    /** Name of the main child slot (the one wrap/unwrap/split/merge act on). */
    main: string | null;
}

/** Keys the JSON writer adds for source tracking; copied, never diffed. */
export const ANNOTATION_KEYS = [
    's', 'a', 'l', 'targetS', 'textS', 'captionS', 'headS', 'bodiesS', 'footS',
] as const;

/** Custom nodes use a slot named `content` as their main child list. */
const CUSTOM_MAIN_SLOT = 'content';

type Obj = Record<string, unknown>;
type Arr = unknown[];

// --- accessor helpers ------------------------------------------------------

function field(holder: Obj | Arr, key: string | number): PropAccess {
    const h = holder as Record<string | number, unknown>;
    return { get: () => h[key], set: (v) => { h[key] = v; } };
}

function listAt(name: string, elem: Elem, holder: Obj | Arr, key: string | number): SlotAccess {
    const h = holder as Record<string | number, unknown>;
    return {
        name,
        elem,
        read: () => (Array.isArray(h[key]) ? (h[key] as Arr) : []),
        write: () => {
            if (!Array.isArray(h[key])) h[key] = [];
            return h[key] as Arr;
        },
        replace: (next) => { h[key] = next; },
    };
}

function selfList(name: string, elem: Elem, value: Arr): SlotAccess {
    return { name, elem, read: () => value, write: () => value };
}

/** A fixed single child (table head/foot) presented as a one-element list. */
function single(name: string, elem: Elem, holder: Arr, key: number): SlotAccess {
    return {
        name,
        elem,
        read: () => [holder[key]],
        write: () => [holder[key]],
    };
}

function leaf(kind: string, elem: Elem, props: Record<string, PropAccess> = {}, text: PropAccess | null = null, attr: PropAccess | null = null): Desc {
    return { kind, elem, attr, props, text, slots: [], main: null };
}

function container(kind: string, elem: Elem, slots: SlotAccess[], main: string | null, opts: { attr?: PropAccess | null; props?: Record<string, PropAccess>; text?: PropAccess | null } = {}): Desc {
    return { kind, elem, attr: opts.attr ?? null, props: opts.props ?? {}, text: opts.text ?? null, slots, main };
}

function captionSlots(caption: Arr): SlotAccess[] {
    return [listAt('caption_short', 'inline', caption, 0), listAt('caption_long', 'block', caption, 1)];
}

function customDesc(kind: string, elem: Elem, node: Obj): Desc {
    const slotsRec = (node.slots ?? {}) as Record<string, { kind: string; value: unknown }>;
    const slots: SlotAccess[] = [];
    let main: string | null = null;
    for (const [name, slot] of Object.entries(slotsRec)) {
        const childElem: Elem = slot.kind === 'block' || slot.kind === 'blocks' ? 'block' : 'inline';
        if (slot.kind === 'blocks' || slot.kind === 'inlines') {
            slots.push(listAt(name, childElem, slot, 'value'));
            if (name === CUSTOM_MAIN_SLOT) main = name;
        } else {
            slots.push(single(name, childElem, [slot.value], 0));
        }
    }
    return container(kind, elem, slots, main, {
        attr: field(node, 'attr'),
        props: { type_name: field(node, 'type_name'), plain_data: field(node, 'plain_data') },
    });
}

// --- the table -------------------------------------------------------------

export function describe(value: unknown, elem: Elem): Desc {
    switch (elem) {
        case 'block': return describeBlock(value as Obj);
        case 'inline': return describeInline(value as Obj);
        case 'list_item': return container('ListItem', elem, [selfList('content', 'block', value as Arr)], 'content');
        case 'line': return container('Line', elem, [selfList('content', 'inline', value as Arr)], 'content');
        case 'definition': return container('Definition', elem, [selfList('content', 'block', value as Arr)], 'content');
        case 'definition_item': {
            const v = value as Arr;
            return container('DefinitionItem', elem, [listAt('term', 'inline', v, 0), listAt('definitions', 'definition', v, 1)], 'definitions');
        }
        case 'citation': {
            const v = value as Obj;
            return container('Citation', elem, [listAt('prefix', 'inline', v, 'citationPrefix'), listAt('suffix', 'inline', v, 'citationSuffix')], null, {
                props: {
                    id: field(v, 'citationId'),
                    mode: field(v, 'citationMode'),
                    note_num: field(v, 'citationNoteNum'),
                    hash: field(v, 'citationHash'),
                },
            });
        }
        case 'table_head': {
            const v = value as Arr;
            return container('TableHead', elem, [listAt('rows', 'row', v, 1)], 'rows', { attr: field(v, 0) });
        }
        case 'table_foot': {
            const v = value as Arr;
            return container('TableFoot', elem, [listAt('rows', 'row', v, 1)], 'rows', { attr: field(v, 0) });
        }
        case 'table_body': {
            const v = value as Arr;
            return container('TableBody', elem, [listAt('head', 'row', v, 2), listAt('body', 'row', v, 3)], 'body', {
                attr: field(v, 0),
                props: { rowhead_columns: field(v, 1) },
            });
        }
        case 'row': {
            const v = value as Arr;
            return container('Row', elem, [listAt('cells', 'cell', v, 1)], 'cells', { attr: field(v, 0) });
        }
        case 'cell': {
            const v = value as Arr;
            return container('Cell', elem, [listAt('content', 'block', v, 4)], 'content', {
                attr: field(v, 0),
                props: { alignment: field(v, 1), row_span: field(v, 2), col_span: field(v, 3) },
            });
        }
    }
}

function describeBlock(node: Obj): Desc {
    const t = String(node.t);
    const c = node.c as Arr;
    const e: Elem = 'block';
    switch (t) {
        case 'Plain':
            return container(t, e, [listAt('content', 'inline', node, 'c')], 'content');
        case 'Para':
            return container(t, e, [listAt('content', 'inline', node, 'c')], 'content', { props: { attr: field(node, 'attr') } });
        case 'LineBlock':
            return container(t, e, [listAt('lines', 'line', node, 'c')], 'lines');
        case 'CodeBlock':
            return leaf(t, e, {}, field(c, 1), field(c, 0));
        case 'RawBlock':
            return leaf(t, e, { format: field(c, 0) }, field(c, 1));
        case 'BlockQuote':
            return container(t, e, [listAt('content', 'block', node, 'c')], 'content');
        case 'OrderedList': {
            const la = c[0] as Arr;
            return container(t, e, [listAt('items', 'list_item', c, 1)], 'items', {
                props: { start: field(la, 0), style: field(la, 1), delim: field(la, 2), itemAttr: field(node, 'itemAttr') },
            });
        }
        case 'BulletList':
            return container(t, e, [listAt('items', 'list_item', node, 'c')], 'items', { props: { itemAttr: field(node, 'itemAttr') } });
        case 'DefinitionList':
            return container(t, e, [listAt('items', 'definition_item', node, 'c')], 'items');
        case 'Header':
            return container(t, e, [listAt('content', 'inline', c, 2)], 'content', { attr: field(c, 1), props: { level: field(c, 0) } });
        case 'HorizontalRule':
            return leaf(t, e);
        case 'Table':
            return container(t, e, [
                ...captionSlots(c[1] as Arr),
                single('head', 'table_head', c, 3),
                listAt('bodies', 'table_body', c, 4),
                single('foot', 'table_foot', c, 5),
            ], null, { attr: field(c, 0), props: { colspec: field(c, 2) } });
        case 'Figure':
            return container(t, e, [listAt('content', 'block', c, 2), ...captionSlots(c[1] as Arr)], 'content', { attr: field(c, 0) });
        case 'Div':
            return container(t, e, [listAt('content', 'block', c, 1)], 'content', { attr: field(c, 0) });
        case 'NoteDefinitionPara':
            return container(t, e, [listAt('content', 'inline', c, 1)], 'content', { props: { id: field(c, 0) } });
        case 'NoteDefinitionFencedBlock':
            return container(t, e, [listAt('content', 'block', c, 1)], 'content', { props: { id: field(c, 0) } });
        case 'CaptionBlock':
            return container(t, e, [listAt('content', 'inline', node, 'c')], 'content');
        case 'CustomBlock':
            return customDesc(t, e, node);
        default:
            // BlockMetadata and anything unknown: opaque payload, compared whole.
            return leaf(t, e, { c: field(node, 'c') });
    }
}

const FLAT_INLINES = new Set(['Emph', 'Underline', 'Strong', 'Strikeout', 'Superscript', 'Subscript', 'SmallCaps']);

function describeInline(node: Obj): Desc {
    const t = String(node.t);
    const c = node.c as Arr;
    const e: Elem = 'inline';
    if (FLAT_INLINES.has(t)) {
        return container(t, e, [listAt('content', 'inline', node, 'c')], 'content');
    }
    switch (t) {
        case 'Str':
            return leaf(t, e, {}, field(node, 'c'));
        case 'Space':
        case 'SoftBreak':
        case 'LineBreak':
            return leaf(t, e);
        case 'Quoted':
            return container(t, e, [listAt('content', 'inline', c, 1)], 'content', { props: { quote_type: field(c, 0) } });
        case 'Cite':
            return container(t, e, [listAt('content', 'inline', c, 1), listAt('citations', 'citation', c, 0)], 'content');
        case 'Code':
            return leaf(t, e, {}, field(c, 1), field(c, 0));
        case 'Math':
            return leaf(t, e, { math_type: field(c, 0) }, field(c, 1));
        case 'RawInline':
            return leaf(t, e, { format: field(c, 0) }, field(c, 1));
        case 'Link':
        case 'Image': {
            const target = c[2] as Arr;
            return container(t, e, [listAt('content', 'inline', c, 1)], 'content', {
                attr: field(c, 0),
                props: { url: field(target, 0), title: field(target, 1) },
            });
        }
        case 'Note':
            return container(t, e, [listAt('content', 'block', node, 'c')], 'content');
        case 'Span':
            return container(t, e, [listAt('content', 'inline', c, 1)], 'content', { attr: field(c, 0) });
        case 'CustomInline':
            return customDesc(t, e, node);
        default:
            return leaf(t, e, { c: field(node, 'c') });
    }
}

// --- views (description + structural hash) ---------------------------------

export interface ViewSlot {
    name: string;
    elem: Elem;
    children: View[];
}

export interface View {
    value: unknown;
    kind: string;
    elem: Elem;
    attr: Attr | null;
    attrJson: string | null;
    /** property name → JSON-encoded value, for cheap comparison */
    propsJson: Record<string, string>;
    props: Record<string, unknown>;
    text: string | null;
    slots: ViewSlot[];
    main: number | null;
    hash: number;
}

function encode(v: unknown): string {
    return v === undefined ? 'undefined' : JSON.stringify(v);
}

/** cyrb53 string hash — fast, 53-bit, good enough for subtree identity. */
function hashString(str: string, seed = 0): number {
    let h1 = 0xdeadbeef ^ seed;
    let h2 = 0x41c6ce57 ^ seed;
    for (let i = 0; i < str.length; i++) {
        const ch = str.charCodeAt(i);
        h1 = Math.imul(h1 ^ ch, 2654435761);
        h2 = Math.imul(h2 ^ ch, 1597334677);
    }
    h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507);
    h1 ^= Math.imul(h2 ^ (h2 >>> 13), 3266489909);
    h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507);
    h2 ^= Math.imul(h1 ^ (h1 >>> 13), 3266489909);
    return 4294967296 * (2097151 & h2) + (h1 >>> 0);
}

function finish(view: Omit<View, 'hash'>): View {
    const parts: string[] = [view.kind, view.elem, view.attrJson ?? '', view.text ?? ''];
    for (const name of Object.keys(view.propsJson).sort()) {
        parts.push(`${name}=${view.propsJson[name]}`);
    }
    for (const slot of view.slots) {
        parts.push(`${slot.name}:${slot.children.length}:${slot.children.map((c) => c.hash).join(',')}`);
    }
    return { ...view, hash: hashString(parts.join('\u0001')) };
}

export function buildView(value: unknown, elem: Elem): View {
    const desc = describe(value, elem);
    const attr = desc.attr ? (desc.attr.get() as Attr) : null;
    const props: Record<string, unknown> = {};
    const propsJson: Record<string, string> = {};
    for (const [name, access] of Object.entries(desc.props)) {
        const v = access.get();
        props[name] = v;
        propsJson[name] = encode(v);
    }
    const textRaw = desc.text ? desc.text.get() : null;
    const slots: ViewSlot[] = desc.slots.map((s) => ({
        name: s.name,
        elem: s.elem,
        children: s.read().map((child) => buildView(child, s.elem)),
    }));
    const main = desc.main === null ? null : slots.findIndex((s) => s.name === desc.main);
    return finish({
        value,
        kind: desc.kind,
        elem,
        attr,
        attrJson: attr === null ? null : encode(attr),
        propsJson,
        props,
        text: typeof textRaw === 'string' ? textRaw : null,
        slots,
        main: main === -1 ? null : main,
    });
}

export function mainSlot(v: View): ViewSlot | null {
    return v.main === null ? null : v.slots[v.main];
}

export function mainChildren(v: View): View[] {
    return mainSlot(v)?.children ?? [];
}

/**
 * The child list a wrapper of `elem` nodes ultimately holds `elem` nodes in,
 * following main slots through single-child intermediates.
 *
 * A Div holds blocks directly (`via: []`, `slot: 'content'`). A one-item
 * bullet list holds blocks two levels down (`via: [items[0]]`,
 * `slot: 'content'`), and so can wrap/unwrap blocks just like a Div. Null
 * when `v` is not an `elem` itself or no such list exists (an intermediate
 * level has several children, or a leaf is reached).
 */
export interface WrapChain {
    via: { slot: string; index: number }[];
    slot: string;
    children: View[];
}

export function wrapChain(v: View, elem: Elem): WrapChain | null {
    if (v.elem !== elem) return null;
    const via: WrapChain['via'] = [];
    let cur = v;
    for (;;) {
        const m = mainSlot(cur);
        if (!m) return null;
        if (m.elem === elem) return { via, slot: m.name, children: m.children };
        if (m.children.length !== 1) return null;
        via.push({ slot: m.name, index: 0 });
        cur = m.children[0];
    }
}

/**
 * An empty copy of `v`'s node: the same kind, attr and properties, with the
 * main child list replaced by a fresh empty array. Shares no container with
 * the original, so filling it never touches the tree `v` came from.
 */
export function shellOf(v: View): unknown {
    const m = mainSlot(v);
    if (!m) throw new Error(`shellOf(${v.kind}): no main slot`);
    const value = v.value;
    let clone: unknown;
    if (Array.isArray(value)) {
        clone = value.slice();
    } else {
        const obj = value as Record<string, unknown>;
        clone = { ...obj, ...(Array.isArray(obj.c) ? { c: (obj.c as unknown[]).slice() } : {}) };
    }
    const slot = describe(clone, v.elem).slots.find((s) => s.name === m.name)!;
    if (slot.replace) slot.replace([]);
    else (clone as unknown[]).length = 0;
    return clone;
}

/** Synthetic copy with different text (before-side split/merge). */
export function withText(v: View, text: string): View {
    return finish({ ...v, text, slots: [], main: null });
}

/** Synthetic copy with different main children (before-side split/merge). */
export function withMainChildren(v: View, children: View[]): View {
    if (v.main === null) throw new Error('withMainChildren on a node without a main slot');
    const slots = v.slots.map((s, i) => (i === v.main ? { ...s, children } : s));
    return finish({ ...v, slots });
}

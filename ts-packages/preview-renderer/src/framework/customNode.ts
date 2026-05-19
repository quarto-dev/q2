/**
 * Wire-format ↔ JS-native conversion for Quarto CustomNodes.
 *
 * Wire format: Quarto's CustomNodes are encoded as Pandoc Div/Span
 * wrappers carrying `class="__quarto_custom_node"` plus three
 * `data-custom-*` kvs. The Rust writer emits this shape (see
 * `crates/pampa/src/writers/json.rs::write_custom_block:1297` and
 * `write_custom_inline:1381`); the Rust reader decodes it (see
 * `read_custom_block_from_div:2220` and `read_custom_inline_from_span:2358`).
 *
 * JS-native shape: `CustomBlockNode` / `CustomInlineNode` with a
 * `slots: Record<string, Slot>` map (defined in `./types.ts`). The
 * format dispatcher routes `t === 'CustomBlock' | 'CustomInline'`
 * to a per-format CustomNode registry without per-tag knowledge.
 *
 * `unwrapCustomNodes` runs after `JSON.parse(astJson)` in
 * `framework/Ast.tsx`; `rewrapCustomNodes` runs before
 * `postMessage({ type: 'SET_AST' })` in q2-preview's `entry.tsx`.
 *
 * Walker scope: only `c` fields on wire-format nodes, and `slots`
 * on JS-native CustomNodes. Never `plain_data` (it's an opaque
 * payload encoded as a `data-custom-data` JSON string at the
 * wrapper boundary).
 *
 * Walker purity: subtrees that contain no transformable nodes are
 * returned by reference. Only the path from root to a transformed
 * node is rebuilt. This invariant is load-bearing for the Note
 * `WeakMap<NoteInline, number>` lookup q2-preview's PreviewRoot
 * builds against the parsed-but-not-yet-unwrapped AST.
 */

import type {
    Attr,
    BlockNode,
    CustomBlockNode,
    CustomInlineNode,
    InlineNode,
    PandocAST,
    Slot,
} from './types';

const CUSTOM_NODE_CLASS = '__quarto_custom_node';

type SlotKindWire = 'Block' | 'Inline' | 'Blocks' | 'Inlines';

/** Walk the AST and replace every `__quarto_custom_node` Div/Span
 * wrapper with a JS-native `CustomBlockNode` / `CustomInlineNode`. */
export function unwrapCustomNodes(ast: PandocAST): PandocAST {
    const newBlocks = unwrapList(ast.blocks);
    if (newBlocks === ast.blocks) return ast;
    return { ...ast, blocks: newBlocks };
}

/** Walk the AST and replace every JS-native `CustomBlockNode` /
 * `CustomInlineNode` with the wire-format Div/Span wrapper. */
export function rewrapCustomNodes(ast: PandocAST): PandocAST {
    const newBlocks = rewrapList(ast.blocks);
    if (newBlocks === ast.blocks) return ast;
    return { ...ast, blocks: newBlocks };
}

// --- unwrap (wire → JS-native) -----------------------------------------

function unwrapList<T>(items: T[]): T[] {
    let next: T[] = items;
    for (let i = 0; i < items.length; i++) {
        const original = items[i];
        const transformed = unwrapAny(original) as T;
        if (transformed !== (original as unknown)) {
            if (next === items) next = items.slice();
            next[i] = transformed;
        }
    }
    return next;
}

/**
 * Walk one value (node or container). Recurses into arrays element-wise
 * and into `c` fields on Pandoc-shaped objects. Custom-node wrappers
 * are detected and decoded; everything else is returned by reference
 * unless a descendant changed.
 */
function unwrapAny(value: unknown): unknown {
    if (!value || typeof value !== 'object') return value;
    if (Array.isArray(value)) return unwrapArray(value);

    const obj = value as { t?: unknown; c?: unknown; s?: unknown };
    if (typeof obj.t !== 'string') return value;

    if ((obj.t === 'Div' || obj.t === 'Span') && isCustomWrapper(obj)) {
        return decodeWrapper(obj as { t: string; c?: unknown; s?: unknown });
    }

    if ('c' in obj) {
        const newC = unwrapAny(obj.c);
        if (newC !== obj.c) {
            return { ...obj, c: newC };
        }
    }
    return value;
}

function unwrapArray(arr: unknown[]): unknown[] {
    let next: unknown[] = arr;
    for (let i = 0; i < arr.length; i++) {
        const original = arr[i];
        const transformed = unwrapAny(original);
        if (transformed !== original) {
            if (next === arr) next = arr.slice();
            next[i] = transformed;
        }
    }
    return next;
}

function isCustomWrapper(obj: { c?: unknown }): boolean {
    const c = obj.c;
    if (!Array.isArray(c) || c.length < 1) return false;
    const attr = c[0];
    if (!Array.isArray(attr) || attr.length < 2) return false;
    const classes = attr[1];
    if (!Array.isArray(classes)) return false;
    return classes.includes(CUSTOM_NODE_CLASS);
}

function decodeWrapper(wrapper: {
    t: string;
    c?: unknown;
    s?: unknown;
}): CustomBlockNode | CustomInlineNode {
    const isBlock = wrapper.t === 'Div';
    const c = wrapper.c as [unknown, unknown];
    const wireAttr = c[0] as [string, string[], [string, string][]];
    const slotChildren = (c[1] as unknown[]) ?? [];

    const [id, classes, kvs] = wireAttr;

    // Lookup helper for kvs (preserves order on the cleaned copy below).
    let typeName = 'Unknown';
    let slotsMetaJson = '{}';
    let plainDataJson: string | null = null;
    for (const [k, v] of kvs) {
        if (k === 'data-custom-type') typeName = v;
        else if (k === 'data-custom-slots') slotsMetaJson = v;
        else if (k === 'data-custom-data') plainDataJson = v;
    }
    const slotMeta = parseJsonOr(slotsMetaJson, {}) as Record<string, SlotKindWire>;
    const plainData = plainDataJson === null ? null : parseJsonOr(plainDataJson, null);

    // Strip custom-node leakage from the wrapper's attr to get the
    // user-supplied attr. Class order and kv order of the *remaining*
    // entries are preserved; this matches the Rust reader's behavior.
    const cleanClasses = classes.filter((cl) => cl !== CUSTOM_NODE_CLASS);
    const cleanKvs: [string, string][] = kvs.filter(
        ([k]) =>
            k !== 'data-custom-type' &&
            k !== 'data-custom-slots' &&
            k !== 'data-custom-data',
    );
    const attr: Attr = [id, cleanClasses, cleanKvs];

    // Slot iteration order is the order children appear in the wire
    // format. Use a plain object to preserve insertion order.
    const slots: Record<string, Slot> = {};
    const expectedSlotTag = isBlock ? 'Div' : 'Span';

    for (const slotChild of slotChildren) {
        if (!slotChild || typeof slotChild !== 'object') continue;
        const sc = slotChild as {
            t?: unknown;
            c?: unknown;
        };
        if (sc.t !== expectedSlotTag) continue;
        if (!Array.isArray(sc.c) || sc.c.length < 2) continue;
        const slotAttr = sc.c[0];
        if (!Array.isArray(slotAttr) || slotAttr.length < 3) continue;
        const slotKvs = slotAttr[2] as [string, string][];
        if (!Array.isArray(slotKvs)) continue;

        let slotName: string | null = null;
        for (const [k, v] of slotKvs) {
            if (k === 'data-slot-name') {
                slotName = v;
                break;
            }
        }
        if (slotName === null) continue;

        const slotContent = sc.c[1] as unknown[];
        if (!Array.isArray(slotContent)) continue;

        // Default kind mirrors Rust reader at :2298 (block) and :2436 (inline).
        const declaredKind: SlotKindWire =
            slotMeta[slotName] ?? (isBlock ? 'Blocks' : 'Inlines');

        const decoded = decodeSlotContent(isBlock, declaredKind, slotContent);
        if (decoded) slots[slotName] = decoded;
    }

    const result: CustomBlockNode | CustomInlineNode = isBlock
        ? ({ t: 'CustomBlock', type_name: typeName, slots, plain_data: plainData, attr } as CustomBlockNode)
        : ({ t: 'CustomInline', type_name: typeName, slots, plain_data: plainData, attr } as CustomInlineNode);
    if (typeof wrapper.s === 'number') {
        (result as { s?: number }).s = wrapper.s;
    }
    return result;
}

function decodeSlotContent(
    isBlock: boolean,
    declared: SlotKindWire,
    slotContent: unknown[],
): Slot | null {
    if (isBlock) {
        switch (declared) {
            case 'Block': {
                const v = slotContent[0];
                if (!v) return null;
                return { kind: 'block', value: unwrapAny(v) as BlockNode };
            }
            case 'Inline': {
                // Wire format: slotContent[0] is a Plain block wrapping
                // the inline (writer at json.rs:1340).
                const plain = slotContent[0] as { t?: unknown; c?: unknown } | undefined;
                if (
                    !plain ||
                    plain.t !== 'Plain' ||
                    !Array.isArray(plain.c) ||
                    plain.c.length === 0
                ) {
                    return null;
                }
                return { kind: 'inline', value: unwrapAny(plain.c[0]) as InlineNode };
            }
            case 'Blocks':
                return { kind: 'blocks', value: unwrapList(slotContent) as BlockNode[] };
            case 'Inlines': {
                const plain = slotContent[0] as { t?: unknown; c?: unknown } | undefined;
                if (!plain || plain.t !== 'Plain' || !Array.isArray(plain.c)) return null;
                return { kind: 'inlines', value: unwrapList(plain.c) as InlineNode[] };
            }
        }
    } else {
        switch (declared) {
            case 'Inline': {
                const v = slotContent[0];
                if (!v) return null;
                return { kind: 'inline', value: unwrapAny(v) as InlineNode };
            }
            case 'Inlines':
                return { kind: 'inlines', value: unwrapList(slotContent) as InlineNode[] };
            // Inline wrapper with a Block/Blocks slot is degenerate
            // (Q-3-39 — block slot in inline custom node). Mirror Rust
            // reader at json.rs:2453 and treat as Inlines.
            case 'Block':
            case 'Blocks':
                return { kind: 'inlines', value: unwrapList(slotContent) as InlineNode[] };
        }
    }
    return null;
}

function parseJsonOr<T>(json: string, fallback: T): T {
    try {
        return JSON.parse(json) as T;
    } catch {
        return fallback;
    }
}

// --- rewrap (JS-native → wire) -----------------------------------------

function rewrapList<T>(items: T[]): T[] {
    let next: T[] = items;
    for (let i = 0; i < items.length; i++) {
        const original = items[i];
        const transformed = rewrapAny(original) as T;
        if (transformed !== (original as unknown)) {
            if (next === items) next = items.slice();
            next[i] = transformed;
        }
    }
    return next;
}

function rewrapAny(value: unknown): unknown {
    if (!value || typeof value !== 'object') return value;
    if (Array.isArray(value)) return rewrapArray(value);

    const obj = value as { t?: unknown; c?: unknown };
    if (typeof obj.t !== 'string') return value;

    if (obj.t === 'CustomBlock' || obj.t === 'CustomInline') {
        return encodeWrapper(value as CustomBlockNode | CustomInlineNode);
    }

    if ('c' in obj) {
        const newC = rewrapAny(obj.c);
        if (newC !== obj.c) {
            return { ...obj, c: newC };
        }
    }
    return value;
}

function rewrapArray(arr: unknown[]): unknown[] {
    let next: unknown[] = arr;
    for (let i = 0; i < arr.length; i++) {
        const original = arr[i];
        const transformed = rewrapAny(original);
        if (transformed !== original) {
            if (next === arr) next = arr.slice();
            next[i] = transformed;
        }
    }
    return next;
}

function encodeWrapper(node: CustomBlockNode | CustomInlineNode): unknown {
    const isBlock = node.t === 'CustomBlock';
    const wrapperTag = isBlock ? 'Div' : 'Span';
    const slotTag = isBlock ? 'Div' : 'Span';

    // Build slot_meta from the JS-native slot kinds.
    const slotMeta: Record<string, SlotKindWire> = {};
    for (const [name, slot] of Object.entries(node.slots)) {
        slotMeta[name] = capitalizedKind(slot.kind);
    }

    // Compose wrapper attr. Class order: __quarto_custom_node first,
    // user classes after. Kv order: user kvs first, then data-custom-*.
    const [id, userClasses, userKvs] = node.attr;
    const wrapperClasses = [CUSTOM_NODE_CLASS, ...userClasses];
    const wrapperKvs: [string, string][] = [
        ...userKvs,
        ['data-custom-type', node.type_name],
        ['data-custom-slots', JSON.stringify(slotMeta)],
    ];
    if (node.plain_data !== null && node.plain_data !== undefined) {
        wrapperKvs.push(['data-custom-data', JSON.stringify(node.plain_data)]);
    }
    const wrapperAttr: Attr = [id, wrapperClasses, wrapperKvs];

    // Encode each slot. Iteration order is preserved by Object.entries.
    const slotWrappers: unknown[] = [];
    for (const [name, slot] of Object.entries(node.slots)) {
        const slotAttr: Attr = ['', [], [['data-slot-name', name]]];
        const slotContent = encodeSlotContent(isBlock, slot);
        slotWrappers.push({
            t: slotTag,
            c: [slotAttr, slotContent],
        });
    }

    const wireWrapper: Record<string, unknown> = {
        t: wrapperTag,
        c: [wrapperAttr, slotWrappers],
    };
    if (typeof node.s === 'number') wireWrapper.s = node.s;
    return wireWrapper;
}

function encodeSlotContent(isBlock: boolean, slot: Slot): unknown[] {
    if (isBlock) {
        switch (slot.kind) {
            case 'block':
                return [rewrapAny(slot.value)];
            case 'inline':
                // Wrap single inline in Plain (writer json.rs:1340).
                return [{ t: 'Plain', c: [rewrapAny(slot.value)] }];
            case 'blocks':
                return rewrapList(slot.value);
            case 'inlines':
                // Wrap inlines array in Plain (writer json.rs:1351).
                return [{ t: 'Plain', c: rewrapList(slot.value) }];
        }
    } else {
        switch (slot.kind) {
            case 'inline':
                return [rewrapAny(slot.value)];
            case 'inlines':
                return rewrapList(slot.value);
            // Block/Blocks in an inline wrapper isn't round-trippable;
            // mirror Rust writer at json.rs:1438 and emit a placeholder.
            case 'block':
            case 'blocks':
                return [{ t: 'Str', c: '[block content]' }];
        }
    }
}

function capitalizedKind(kind: Slot['kind']): SlotKindWire {
    switch (kind) {
        case 'block':
            return 'Block';
        case 'inline':
            return 'Inline';
        case 'blocks':
            return 'Blocks';
        case 'inlines':
            return 'Inlines';
    }
}

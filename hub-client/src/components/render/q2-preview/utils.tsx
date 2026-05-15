/**
 * q2-preview format-specific helpers.
 *
 *  - `lookupAssetUrl`: external-URL passthrough + manifest lookup with
 *    fallback to the original URL on miss (broken-image affordance).
 *  - `formatRefLabel`: Theorem-style "Theorem 1 (Pythagoras)" labels.
 *  - `composeAttr`: non-mutating add-classes / add-kvs onto an Attr.
 *  - `renderSlot`: slot-kind dispatcher that mounts <Node> with a
 *    copy-on-write `setLocalAst` for each slot value. Mirrors
 *    `renderCustomNodeChildren` in `framework/dispatch.tsx` but is the
 *    per-slot helper a per-type CustomNode component drives directly.
 *  - `makeSlotSetter`: factory that turns a CustomNode + setLocalAst
 *    into a `(slotName) => (newSlot) => void` so a per-type component
 *    doesn't repeat the `{ ...node, slots: { ...node.slots, [name]: x } }`
 *    spread for every slot it renders.
 *
 * The Pandoc-AST plain-text walks (`inlinesToPlainText` /
 * `blocksToPlainText`) used to live here; they moved to
 * `framework/plainText.ts` so framework-tier consumers
 * (meta-string coercion, the slide renderer) can use them
 * without a layering inversion. Q2-preview consumers import
 * them via the `framework` barrel.
 */

import type { ReactNode } from 'react';
import type {
    BlockNode,
    CustomBlockNode,
    CustomInlineNode,
    InlineNode,
    Slot,
} from '../framework';
import type { Attr } from '../framework/types';
import { Node } from '../framework';

// --- asset URL lookup ------------------------------------------------

/**
 * Resolve an Image `target.0` URL string to the URL the iframe should
 * actually load. External URLs (`https?:`, `data:`, `//`) pass through
 * unchanged; project-relative paths look up in the manifest. On
 * manifest miss, the original URL is returned — the resulting broken
 * `<img>` is a deliberate signal that resolution failed (silently
 * swallowing missing images would hide bugs in the walker).
 */
export function lookupAssetUrl(
    manifest: Record<string, string>,
    url: string,
): string {
    if (
        url.startsWith('https://') ||
        url.startsWith('http://') ||
        url.startsWith('data:') ||
        url.startsWith('//')
    ) {
        return url;
    }
    return manifest[url] ?? url;
}

// --- CustomNode helpers ---------------------------------------------

/**
 * Compose a "Theorem 1 (Pythagoras)"-style label.
 *
 * - `kind` empty → empty string (the consumer is expected to drop the
 *   label entirely; matches Rust's `theorem_label_inlines` behavior
 *   when the kind is missing).
 * - `number` numeric → "{kind}\u{00A0}{number}" (NBSP joins kind +
 *   number so the label doesn't line-wrap; matches
 *   `crossref_render.rs:450`).
 * - `number` undefined → "{kind}" alone (number-elision branch;
 *   `crossref_render.rs:444-453`).
 * - `title` non-empty → " (title)" appended (matches the literal
 *   `" ("` / `")"` Rust wraps the title inlines in).
 *
 * Used by Theorem; FloatRefTarget / CrossrefResolvedRef build their
 * labels inline because they have different separators (ASCII space
 * vs NBSP) and different wrapping (figcaption vs anchor body) that
 * make a shared helper noisier than direct template strings.
 */
export function formatRefLabel(
    kind: string,
    number: number | undefined,
    title?: string,
): string {
    if (!kind) return '';
    let label = number !== undefined ? `${kind} ${number}` : kind;
    if (title && title.length > 0) label += ` (${title})`;
    return label;
}

/**
 * Non-mutating Attr extension. Appends `extraClasses` to the classes
 * array (preserving order, with no de-dup — match Rust's `make_attr`)
 * and `extraKvs` to the key/value pairs. The original Attr is not
 * mutated; both inner arrays are copied.
 *
 * Useful for components that need to layer their built-in classes
 * (e.g. `theorem`, `quarto-xref`) on top of the user's authored attr
 * without aliasing the input.
 */
export function composeAttr(
    original: Attr,
    extraClasses: string[],
    extraKvs: [string, string][] = [],
): Attr {
    const [id, classes, kvs] = original;
    return [
        id,
        extraClasses.length > 0 ? [...classes, ...extraClasses] : [...classes],
        extraKvs.length > 0 ? [...kvs, ...extraKvs] : [...kvs],
    ];
}

/**
 * Per-slot recursion helper for CustomNode components.
 *
 * `setSlot` replaces the named slot on the parent CustomNode and
 * propagates the rewritten parent up via the framework-supplied
 * `setLocalAst`. The shape matches `renderCustomNodeChildren` in
 * `framework/dispatch.tsx` (which is the generic-walk fallback used
 * by `Fallback.tsx`); the duplication is intentional — the framework
 * registry entry walks all slots indiscriminately, while per-type
 * components drive their own per-slot rendering with named slot keys
 * (Callout's `title` / `content`, FloatRefTarget's `caption_long` /
 * `caption_short`, etc.).
 *
 * The `ctx` arg carries any extra props the caller wants forwarded to
 * `<Node>`; the only one currently honored by `Node` is
 * `onNavigateToDocument`. Future framework props would land here.
 */
export type RenderSlotContext = {
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
};

export function renderSlot(
    slot: Slot | undefined,
    setSlot: (next: Slot) => void,
    ctx: RenderSlotContext = {},
): ReactNode {
    if (!slot) return null;
    switch (slot.kind) {
        case 'block':
            return (
                <Node
                    node={slot.value}
                    onNavigateToDocument={ctx.onNavigateToDocument}
                    setLocalAst={(n) =>
                        setSlot({ kind: 'block', value: n as BlockNode })
                    }
                />
            );
        case 'inline':
            return (
                <Node
                    node={slot.value}
                    onNavigateToDocument={ctx.onNavigateToDocument}
                    setLocalAst={(n) =>
                        setSlot({ kind: 'inline', value: n as InlineNode })
                    }
                />
            );
        case 'blocks':
            return slot.value.map((b, i) => (
                <Node
                    key={i}
                    node={b}
                    onNavigateToDocument={ctx.onNavigateToDocument}
                    setLocalAst={(n) => {
                        const next = slot.value.slice();
                        next[i] = n as BlockNode;
                        setSlot({ kind: 'blocks', value: next });
                    }}
                />
            ));
        case 'inlines':
            return slot.value.map((inl, i) => (
                <Node
                    key={i}
                    node={inl}
                    onNavigateToDocument={ctx.onNavigateToDocument}
                    setLocalAst={(n) => {
                        const next = slot.value.slice();
                        next[i] = n as InlineNode;
                        setSlot({ kind: 'inlines', value: next });
                    }}
                />
            ));
    }
}

/**
 * Build a `(slotName) => (newSlot) => void` setter factory for a
 * CustomNode. The returned per-slot setter rebuilds the parent
 * CustomNode with the named slot replaced and propagates it via the
 * framework-supplied `setLocalAst`.
 *
 * Lifts the `{ ...node, slots: { ...node.slots, [name]: next } }`
 * spread out of every per-type component so it lives in one place.
 */
export function makeSlotSetter(
    node: CustomBlockNode | CustomInlineNode,
    setLocalAst: (newNode: BlockNode | InlineNode) => void,
): (slotName: string) => (next: Slot) => void {
    return (slotName: string) => (next: Slot) => {
        setLocalAst({
            ...node,
            slots: { ...node.slots, [slotName]: next },
        });
    };
}

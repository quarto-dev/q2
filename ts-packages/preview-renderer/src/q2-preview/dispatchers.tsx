import React, { useContext } from 'react';
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
import { sliceBytes } from '../utils/sliceSource';

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
            <div ref={activeEditRegionRef} style={wrapperStyle}>{textarea}</div>
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
 */
function isBlockEditTarget(
    ctx: PreviewContextValue | null,
    poolId: string | number | undefined,
    resolved: ResolvedSource | null,
): boolean {
    return (
        resolved != null &&
        resolved.reachabilityClass !== 'Opaque' &&
        poolId !== undefined &&
        ctx?.commitTextEdit !== undefined &&
        ctx?.content != null &&
        ctx?.editTarget?.poolId === poolId
    );
}

/** Render an in-place editing textarea for the matched block. */
function renderBlockTextarea(
    ctx: PreviewContextValue,
    resolved: ResolvedSource,
): React.ReactNode {
    const { contentHeight } = ctx.editTarget!;
    const initialText = sliceBytes(
        ctx.content!,
        resolved.sourceEntry.r[0],
        resolved.sourceEntry.r[1],
    ).trimEnd();

    const commit = (el: HTMLTextAreaElement) => {
        const text = el.value;
        if (!text.trim()) {
            ctx.setEditTarget!(null);
            return;
        }
        ctx.commitTextEdit!(JSON.stringify(resolved.sourceEntry), text);
        ctx.setEditTarget!(null);
    };

    return (
        <textarea
            autoFocus
            defaultValue={initialText}
            style={{
                display: 'block',
                fontFamily: 'monospace',
                fontSize: '0.9em',
                width: '100%',
                height: contentHeight,
                boxSizing: 'border-box',
                resize: 'vertical',
            }}
            onBlur={(e) => commit(e.currentTarget)}
            onKeyDown={(e) => {
                if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                    e.preventDefault();
                    commit(e.currentTarget);
                } else if (e.key === 'Escape') {
                    e.preventDefault();
                    ctx.setEditTarget!(null);
                }
            }}
        />
    );
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
    const poolId = (args.node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;

    if (isBlockEditTarget(ctx, poolId, resolved) && ctx) {
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
    const poolId = (args.node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;

    if (isBlockEditTarget(ctx, poolId, resolved) && ctx) {
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

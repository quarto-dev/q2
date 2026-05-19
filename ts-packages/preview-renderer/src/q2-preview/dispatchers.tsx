import { useContext } from 'react';
import { RegistryContext, AttributionWrap, renderChildren } from '../framework';
import type {
    BlockNode,
    CustomBlockNode,
    CustomInlineNode,
    InlineNode,
    NodeArgs,
} from '../framework';

const placeholderStyle: React.CSSProperties = {
    color: '#888',
    fontStyle: 'italic',
};

const PLACEHOLDER_CLASS = 'q2-preview-placeholder';

/**
 * q2-preview's Block dispatcher. Looks up the format registry by Pandoc
 * tag and renders the corresponding leaf component, falling back to a
 * muted-gray "(not yet implemented)" placeholder when no component is
 * registered.
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
    const Component =
        registry[args.node.type_name] ?? registry['__fallback__'];
    const inner = <Component {...args} />;

    return <AttributionWrap node={args.node} as="div">{inner}</AttributionWrap>;
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

import { useContext } from 'react';
import { RegistryContext } from '../framework/RegistryContext';
import { renderChildren } from '../framework';
import type { BlockNode, InlineNode, NodeArgs } from '../framework/types';

const placeholderStyle: React.CSSProperties = {
    color: '#888',
    fontStyle: 'italic',
};

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
 */
export const Block = (args: NodeArgs<BlockNode>) => {
    const { registry } = useContext(RegistryContext);
    const Component = registry[args.node.t];
    if (Component) return <Component {...args} />;
    return (
        <div style={placeholderStyle}>
            {args.node.t} (not yet implemented){renderChildren(args)}
        </div>
    );
};

/**
 * q2-preview's Inline dispatcher. Same pattern as `Block` for
 * inline-level nodes — placeholder + recursion on miss so nested
 * inlines surface their own placeholders.
 */
export const Inline = (args: NodeArgs<InlineNode>) => {
    const { registry } = useContext(RegistryContext);
    const Component = registry[args.node.t];
    if (Component) return <Component {...args} />;
    return (
        <span style={placeholderStyle}>
            {args.node.t} (not yet implemented){renderChildren(args)}
        </span>
    );
};

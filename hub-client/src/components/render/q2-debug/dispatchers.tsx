import { useContext } from 'react';
import { RegistryContext } from '../framework/RegistryContext';
import type { BlockNode, InlineNode, NodeArgs } from '../framework/types';
import { blockStyle, inlineStyle } from './styles';

/**
 * q2-debug Block dispatcher: looks up the format registry by Pandoc tag
 * and renders the corresponding leaf component, falling back to a bordered
 * "Not registered" message when no component is registered for the tag.
 */
export const Block = (args: NodeArgs<BlockNode>) => {
    const { registry } = useContext(RegistryContext);

    const Component = registry[args.node.t];
    return Component ? <Component {...args} /> : <div style={blockStyle}><strong>Not registered: {args.node.t}</strong></div>;
}

/**
 * q2-debug Inline dispatcher: same as Block but for inline-level nodes,
 * with the inline-flavored "Not registered" miss path.
 */
export const Inline = (args: NodeArgs<InlineNode>) => {
    const { registry } = useContext(RegistryContext);

    const Component = registry[args.node.t];
    return Component ? <Component {...args} /> : <span style={inlineStyle}><strong>Not registered: {args.node.t}</strong></span>;
}

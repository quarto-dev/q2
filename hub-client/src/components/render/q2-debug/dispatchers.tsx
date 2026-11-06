import { useContext } from 'react';
import { RegistryContext, AttributionWrap } from '@quarto/preview-renderer/framework';
import type { BlockNode, InlineNode, NodeArgs } from '@quarto/preview-renderer/framework';
import { DebugNode } from './components';

/**
 * q2-debug Block dispatcher: looks up the format registry by Pandoc tag
 * and renders the registered component (a user override, or the per-tag
 * entry), falling back to the generic inspector node for tags nobody
 * registered — every tag is inspectable, registered or not.
 *
 * Phase 5c — when attribution is on for this node, `AttributionWrap`
 * emits a `.q2-attr-wrap` div carrying `data-sid` + inline `color`
 * so the descendant text inherits the author's identity colour.
 * Off path the wrap is a pass-through.
 */
export const Block = (args: NodeArgs<BlockNode>) => {
    const { registry } = useContext(RegistryContext);
    const Component = registry[args.node.t];
    const inner = Component ? <Component {...args} /> : <DebugNode {...args} />;
    return <AttributionWrap node={args.node} as="div">{inner}</AttributionWrap>;
}

/**
 * q2-debug Inline dispatcher: same as Block but for inline-level nodes,
 * with a `<span>` attribution wrap.
 */
export const Inline = (args: NodeArgs<InlineNode>) => {
    const { registry } = useContext(RegistryContext);
    const Component = registry[args.node.t];
    const inner = Component ? <Component {...args} /> : <DebugNode {...args} />;
    return <AttributionWrap node={args.node} as="span">{inner}</AttributionWrap>;
}

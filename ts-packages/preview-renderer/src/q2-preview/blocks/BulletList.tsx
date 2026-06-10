import { useContext } from 'react';
import { Node, renderChildren } from '../../framework';
import type { BulletListBlock, NodeArgs } from '../../framework';
import { IncrementalContext } from '../IncrementalContext';
import { PreviewContext } from '../PreviewContext';

const NOOP = () => {};

/** BulletList → <ul>. Outside a revealjs deck the framework's
 * `renderChildrenRegistry.BulletList` wraps each item in an <li> (and threads
 * `setLocalAst` for editing). Inside an incremental revealjs context the
 * component renders the <li>s itself so each gets `class="fragment"` — list
 * items have no AST attr, so the class is attached here (mirrors the native
 * writer). */
export const BulletList = (args: NodeArgs<BulletListBlock>) => {
    const { enabled, incremental } = useContext(IncrementalContext);
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;
    if (enabled) {
        const liClass = incremental ? 'fragment' : undefined;
        return (
            <ul>
                {args.node.c.map((item, i) => (
                    <li key={i} className={liClass}>
                        {item.map((block, j) => (
                            <Node
                                key={`${i}:${j}`}
                                node={block}
                                onNavigateToDocument={args.onNavigateToDocument}
                                setLocalAst={NOOP}
                            />
                        ))}
                    </li>
                ))}
            </ul>
        );
    }
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined;
    return (
        <ul {...(isEditable ? { 'data-block-pool-id': poolId } : {})}>
            {renderChildren(args)}
        </ul>
    );
};

import { useContext } from 'react';
import { Node, liItemAttrProps } from '../../framework';
import type { NodeArgs, OrderedListBlock } from '../../framework';
import { IncrementalContext } from '../IncrementalContext';
import { PreviewContext } from '../PreviewContext';
import { isLeadingBlockBorrowable } from './listBorrow';

const NOOP = () => {};

/**
 * OrderedList → <ol> with `start`, `type`, and `data-list-style-delim`
 * attrs reflecting Pandoc's ListAttributes triple.
 *
 * Pandoc-style mapping (HTML writer):
 *   - DefaultStyle / Decimal: no `type` attr (browser default).
 *   - LowerRoman:  type="i"
 *   - UpperRoman:  type="I"
 *   - LowerAlpha:  type="a"
 *   - UpperAlpha:  type="A"
 *   - Example: no `type` attr (Pandoc renders these specially; we fall
 *     back to default ordering).
 *
 * Inside an incremental revealjs context each <li> gets `class="fragment"`
 * (see `BulletList` for the rationale). A per-item block attr (`itemAttr[i]`,
 * bd-aeyss6p5) composes with it and is applied to every <li> in both paths.
 */
const styleToType: Record<string, string | undefined> = {
    LowerRoman: 'i',
    UpperRoman: 'I',
    LowerAlpha: 'a',
    UpperAlpha: 'A',
};

export const OrderedList = (args: NodeArgs<OrderedListBlock>) => {
    const { enabled, incremental } = useContext(IncrementalContext);
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined && !ctx?.editingDisabled;

    const [[start, style]] = args.node.c;
    const props: Record<string, string | number> = {};
    if (start && start !== 1) props.start = start;
    const typeAttr = styleToType[style.t];
    if (typeAttr) props.type = typeAttr;
    const olProps = props as React.OlHTMLAttributes<HTMLOListElement>;

    if (enabled) {
        return (
            <ol {...olProps}>
                {args.node.c[1].map((item, i) => (
                    <li key={i} {...liItemAttrProps(args.node.itemAttr?.[i], incremental)}>
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
            </ol>
        );
    }

    if (isEditable) {
        props['data-block-pool-id'] = poolId!;
        props.tabIndex = -1;
    }
    return (
        <ol {...olProps}>
            {args.node.c[1].map((item, i) => {
                // Per-item block attr (bd-aeyss6p5) applies to every <li>.
                const itemAttrProps = liItemAttrProps(args.node.itemAttr?.[i], false);
                // Empty-item guard: render a bare <li> to avoid accessing item[0].s on an empty array.
                if (item.length === 0) {
                    return <li key={i} {...itemAttrProps} />;
                }
                // Borrow gate (Amendment A1): only borrow if the leading block is a Plain
                // and it is editable. A Para-leading (loose) item must NOT borrow.
                const leadingBlock = item[0] as any;
                const borrowPoolId = isLeadingBlockBorrowable(leadingBlock, ctx)
                    ? (leadingBlock.s as string | number)
                    : undefined;
                const liProps = borrowPoolId !== undefined
                    ? { 'data-block-pool-id': borrowPoolId, tabIndex: -1 as const }
                    : {};
                return (
                    <li key={i} {...itemAttrProps} {...liProps}>
                        {item.map((block, j) => (
                            <Node
                                key={`${i}:${j}`}
                                node={block}
                                onNavigateToDocument={args.onNavigateToDocument}
                                setLocalAst={NOOP}
                            />
                        ))}
                    </li>
                );
            })}
        </ol>
    );
};

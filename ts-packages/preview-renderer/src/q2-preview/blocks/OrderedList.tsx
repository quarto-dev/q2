import { useContext } from 'react';
import { Node, renderChildren } from '../../framework';
import type { NodeArgs, OrderedListBlock } from '../../framework';
import { IncrementalContext } from '../IncrementalContext';

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
 * (see `BulletList` for the rationale).
 */
const styleToType: Record<string, string | undefined> = {
    LowerRoman: 'i',
    UpperRoman: 'I',
    LowerAlpha: 'a',
    UpperAlpha: 'A',
};

export const OrderedList = (args: NodeArgs<OrderedListBlock>) => {
    const { enabled, incremental } = useContext(IncrementalContext);
    const [[start, style]] = args.node.c;
    const props: Record<string, string | number> = {};
    if (start && start !== 1) props.start = start;
    const typeAttr = styleToType[style.t];
    if (typeAttr) props.type = typeAttr;
    const olProps = props as React.OlHTMLAttributes<HTMLOListElement>;

    if (!enabled) {
        return <ol {...olProps}>{renderChildren(args)}</ol>;
    }
    const liClass = incremental ? 'fragment' : undefined;
    return (
        <ol {...olProps}>
            {args.node.c[1].map((item, i) => (
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
        </ol>
    );
};

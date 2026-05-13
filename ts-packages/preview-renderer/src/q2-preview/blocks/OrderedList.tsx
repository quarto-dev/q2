import { renderChildren } from '../../framework';
import type { NodeArgs, OrderedListBlock } from '../../framework';

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
 */
const styleToType: Record<string, string | undefined> = {
    LowerRoman: 'i',
    UpperRoman: 'I',
    LowerAlpha: 'a',
    UpperAlpha: 'A',
};

export const OrderedList = (args: NodeArgs<OrderedListBlock>) => {
    const [[start, style], ] = args.node.c;
    const props: Record<string, string | number> = {};
    if (start && start !== 1) props.start = start;
    const typeAttr = styleToType[style.t];
    if (typeAttr) props.type = typeAttr;
    return <ol {...(props as React.OlHTMLAttributes<HTMLOListElement>)}>{renderChildren(args)}</ol>;
};

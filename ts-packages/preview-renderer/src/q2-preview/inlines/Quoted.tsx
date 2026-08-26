import { QUOTE_CHARS, renderChildren } from '../../framework';
import type { NodeArgs, QuotedInline } from '../../framework';

export const Quoted = (args: NodeArgs<QuotedInline>) => {
    const [{ t: kind }] = args.node.c;
    const [open, close] = QUOTE_CHARS[kind];
    return (
        <>
            {open}
            {renderChildren(args)}
            {close}
        </>
    );
};

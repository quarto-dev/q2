import { renderChildren } from '../../framework';
import type { BlockQuoteBlock, NodeArgs } from '../../framework';

export const BlockQuote = (args: NodeArgs<BlockQuoteBlock>) => (
    <blockquote>{renderChildren(args)}</blockquote>
);

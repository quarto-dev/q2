import { renderChildren } from '@quarto/preview-renderer/framework';
import type { BlockQuoteBlock, NodeArgs } from '@quarto/preview-renderer/framework';

export const BlockQuote = (args: NodeArgs<BlockQuoteBlock>) => (
    <blockquote>{renderChildren(args)}</blockquote>
);

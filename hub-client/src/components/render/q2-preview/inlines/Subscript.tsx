import { renderChildren } from '@quarto/preview-renderer/framework';
import type { NodeArgs, SubscriptInline } from '@quarto/preview-renderer/framework';

export const Subscript = (args: NodeArgs<SubscriptInline>) => (
    <sub>{renderChildren(args)}</sub>
);

import { renderChildren } from '../../framework';
import type { NodeArgs, SubscriptInline } from '../../framework';

export const Subscript = (args: NodeArgs<SubscriptInline>) => (
    <sub>{renderChildren(args)}</sub>
);

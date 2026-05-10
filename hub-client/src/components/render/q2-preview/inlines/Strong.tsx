import { renderChildren } from '../../framework';
import type { NodeArgs, StrongInline } from '../../framework';

export const Strong = (args: NodeArgs<StrongInline>) => (
    <strong>{renderChildren(args)}</strong>
);

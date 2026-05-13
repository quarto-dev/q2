import { renderChildren } from '../../framework';
import type { NodeArgs, SmallCapsInline } from '../../framework';

export const SmallCaps = (args: NodeArgs<SmallCapsInline>) => (
    <span style={{ fontVariant: 'small-caps' }}>{renderChildren(args)}</span>
);

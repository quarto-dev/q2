import { renderChildren } from '@quarto/preview-renderer/framework';
import type { NodeArgs, SmallCapsInline } from '@quarto/preview-renderer/framework';

export const SmallCaps = (args: NodeArgs<SmallCapsInline>) => (
    <span style={{ fontVariant: 'small-caps' }}>{renderChildren(args)}</span>
);

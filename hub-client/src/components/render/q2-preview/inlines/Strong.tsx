import { renderChildren } from '@quarto/preview-renderer/framework';
import type { NodeArgs, StrongInline } from '@quarto/preview-renderer/framework';

export const Strong = (args: NodeArgs<StrongInline>) => (
    <strong>{renderChildren(args)}</strong>
);

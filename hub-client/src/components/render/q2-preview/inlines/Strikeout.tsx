import { renderChildren } from '@quarto/preview-renderer/framework';
import type { NodeArgs, StrikeoutInline } from '@quarto/preview-renderer/framework';

export const Strikeout = (args: NodeArgs<StrikeoutInline>) => (
    <s>{renderChildren(args)}</s>
);

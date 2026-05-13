import { renderChildren } from '@quarto/preview-renderer/framework';
import type { NodeArgs, UnderlineInline } from '@quarto/preview-renderer/framework';

export const Underline = (args: NodeArgs<UnderlineInline>) => (
    <u>{renderChildren(args)}</u>
);

import { renderChildren } from '@quarto/preview-renderer/framework';
import type { NodeArgs, SuperscriptInline } from '@quarto/preview-renderer/framework';

export const Superscript = (args: NodeArgs<SuperscriptInline>) => (
    <sup>{renderChildren(args)}</sup>
);

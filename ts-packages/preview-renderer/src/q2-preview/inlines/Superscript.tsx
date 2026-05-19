import { renderChildren } from '../../framework';
import type { NodeArgs, SuperscriptInline } from '../../framework';

export const Superscript = (args: NodeArgs<SuperscriptInline>) => (
    <sup>{renderChildren(args)}</sup>
);

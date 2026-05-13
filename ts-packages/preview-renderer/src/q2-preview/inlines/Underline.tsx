import { renderChildren } from '../../framework';
import type { NodeArgs, UnderlineInline } from '../../framework';

export const Underline = (args: NodeArgs<UnderlineInline>) => (
    <u>{renderChildren(args)}</u>
);

import { renderChildren } from '../../framework';
import type { NodeArgs, StrikeoutInline } from '../../framework';

export const Strikeout = (args: NodeArgs<StrikeoutInline>) => (
    <s>{renderChildren(args)}</s>
);

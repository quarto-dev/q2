import { renderChildren } from '../../framework';
import type { NodeArgs, ParaBlock } from '../../framework';

export const Para = (args: NodeArgs<ParaBlock>) => <p>{renderChildren(args)}</p>;

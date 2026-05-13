import { renderChildren } from '@quarto/preview-renderer/framework';
import type { NodeArgs, ParaBlock } from '@quarto/preview-renderer/framework';

export const Para = (args: NodeArgs<ParaBlock>) => <p>{renderChildren(args)}</p>;

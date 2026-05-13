import { renderChildren } from '@quarto/preview-renderer/framework';
import type { NodeArgs, PlainBlock } from '@quarto/preview-renderer/framework';

/** Plain renders no wrapper element — its inlines flow into the
 * surrounding context (table cells, list items, etc.). */
export const Plain = (args: NodeArgs<PlainBlock>) => <>{renderChildren(args)}</>;

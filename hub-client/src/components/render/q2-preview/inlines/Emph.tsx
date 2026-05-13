import { renderChildren } from '@quarto/preview-renderer/framework';
import type { EmphInline, NodeArgs } from '@quarto/preview-renderer/framework';

export const Emph = (args: NodeArgs<EmphInline>) => <em>{renderChildren(args)}</em>;

import { renderChildren } from '../../framework';
import type { EmphInline, NodeArgs } from '../../framework';

export const Emph = (args: NodeArgs<EmphInline>) => <em>{renderChildren(args)}</em>;

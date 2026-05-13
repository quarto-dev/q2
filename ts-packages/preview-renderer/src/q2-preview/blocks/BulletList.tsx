import { renderChildren } from '../../framework';
import type { BulletListBlock, NodeArgs } from '../../framework';

/** BulletList → <ul>. The framework's `renderChildrenRegistry.BulletList`
 * already wraps each item array in an <li>; the component just supplies
 * the <ul> wrapper. */
export const BulletList = (args: NodeArgs<BulletListBlock>) => (
    <ul>{renderChildren(args)}</ul>
);

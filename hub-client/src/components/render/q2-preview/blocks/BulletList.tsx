import { renderChildren } from '@quarto/preview-renderer/framework';
import type { BulletListBlock, NodeArgs } from '@quarto/preview-renderer/framework';

/** BulletList → <ul>. The framework's `renderChildrenRegistry.BulletList`
 * already wraps each item array in an <li>; the component just supplies
 * the <ul> wrapper. */
export const BulletList = (args: NodeArgs<BulletListBlock>) => (
    <ul>{renderChildren(args)}</ul>
);

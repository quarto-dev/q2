import { useContext } from 'react';
import { Node, liItemAttrProps, dataLocProps } from '../../framework';
import type { BulletListBlock, NodeArgs } from '../../framework';
import { IncrementalContext } from '../IncrementalContext';
import { PreviewContext } from '../PreviewContext';
import { isLeadingBlockBorrowable } from './listBorrow';
import { allTaskItems, makeTaskToggle, TaskItemBody, taskItemChecked } from './taskList';

const NOOP = () => {};

/** BulletList → <ul>. Inside an incremental revealjs context the component
 * renders the <li>s itself so each gets `class="fragment"` — list items have no
 * AST attr, so the class is attached here (mirrors the native writer). A
 * per-item block attr (`itemAttr[i]`, bd-aeyss6p5) composes with the `fragment`
 * class.
 *
 * §0 Pool-id borrow: the non-incremental branch also maps items itself so it
 * can borrow the leading Plain block's pool-id onto each <li>. Gate:
 *   item.length > 0 AND item[0].t === 'Plain' AND editable AND !editingDisabled.
 * An empty item (item.length === 0) renders a bare <li> with no pool-id.
 * A Para-leading (loose) item renders a bare <li> so the inner <p> keeps its
 * sole pool-id (Amendment A1 — no duplicate). The per-item block attr
 * (`itemAttr[i]`, bd-aeyss6p5) is still applied to every <li> in both paths. */
export const BulletList = (args: NodeArgs<BulletListBlock>) => {
    const { enabled, incremental } = useContext(IncrementalContext);
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;
    // Pandoc parity: `class="task-list"` iff every item is a task item
    // (bullet lists only — the native writer never puts the class on <ol>).
    const taskListClass = allTaskItems(args.node.c) ? { className: 'task-list' } : {};

    if (enabled) {
        return (
            <ul {...taskListClass} {...dataLocProps(args.node)}>
                {args.node.c.map((item, i) => {
                    const checked = taskItemChecked(item);
                    return (
                        <li key={i} {...liItemAttrProps(args.node.itemAttr?.[i], incremental)}>
                            {checked !== null ? (
                                // Incremental (reveal) surfaces are never edit-enabled:
                                // render the checkbox, but inert.
                                <TaskItemBody
                                    item={item}
                                    checked={checked}
                                    onNavigateToDocument={args.onNavigateToDocument}
                                />
                            ) : (
                                item.map((block, j) => (
                                    <Node
                                        key={`${i}:${j}`}
                                        node={block}
                                        onNavigateToDocument={args.onNavigateToDocument}
                                        setLocalAst={NOOP}
                                    />
                                ))
                            )}
                        </li>
                    );
                })}
            </ul>
        );
    }

    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined && !ctx?.editingDisabled;

    return (
        <ul {...taskListClass} {...(isEditable ? { 'data-block-pool-id': poolId, tabIndex: -1 } : {})} {...dataLocProps(args.node)}>
            {args.node.c.map((item, i) => {
                // Per-item block attr (bd-aeyss6p5) applies to every <li>.
                const itemAttrProps = liItemAttrProps(args.node.itemAttr?.[i], false);
                // Empty-item guard: render a bare <li> to avoid accessing item[0].s on an empty array.
                if (item.length === 0) {
                    return <li key={i} {...itemAttrProps} />;
                }
                // Borrow gate (Amendment A1): only borrow if the leading block is a Plain
                // and it is editable. A Para-leading (loose) item must NOT borrow.
                const leadingBlock = item[0] as any;
                const borrowPoolId = isLeadingBlockBorrowable(leadingBlock, ctx)
                    ? (leadingBlock.s as string | number)
                    : undefined;
                const liProps = borrowPoolId !== undefined
                    ? { 'data-block-pool-id': borrowPoolId, tabIndex: -1 as const }
                    : {};
                const checked = taskItemChecked(item);
                return (
                    <li key={i} {...itemAttrProps} {...liProps}>
                        {checked !== null ? (
                            <TaskItemBody
                                item={item}
                                checked={checked}
                                onToggle={makeTaskToggle(ctx, resolved, i)}
                                onNavigateToDocument={args.onNavigateToDocument}
                            />
                        ) : (
                            item.map((block, j) => (
                                <Node
                                    key={`${i}:${j}`}
                                    node={block}
                                    onNavigateToDocument={args.onNavigateToDocument}
                                    setLocalAst={NOOP}
                                />
                            ))
                        )}
                    </li>
                );
            })}
        </ul>
    );
};

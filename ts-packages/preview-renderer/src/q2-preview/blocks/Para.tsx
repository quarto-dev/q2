import { useContext } from 'react';
import { renderChildren, dataLocProps } from '../../framework';
import type { NodeArgs, ParaBlock } from '../../framework';
import { PreviewContext } from '../PreviewContext';
import { stripTaskMarker, TaskLabel, useTaskItem } from './taskList';

export const Para = (args: NodeArgs<ParaBlock>) => {
    const ctx = useContext(PreviewContext);
    const task = useTaskItem();
    const poolId = (args.node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;

    const isEditable = resolved != null
        && resolved.reachabilityClass !== 'Opaque'
        && poolId !== undefined
        && !ctx?.editingDisabled;

    // A block-level attribute (id/classes/key-values) may ride on the Para node
    // via the optional `attr` key the Rust JSON writer emits (Pandoc ignores
    // it). Apply it to the <p> like Header/Div do, so the preview matches
    // `q2 render` (e.g. `<p class="caption">`). See bd-itqcfxc3.
    const domProps: Record<string, string | number> = {};
    if (args.node.attr) {
        const [id, classes, kvs] = args.node.attr;
        if (id) domProps.id = id;
        if (classes.length) domProps.className = classes.join(' ');
        for (const [k, v] of kvs) {
            if (k.startsWith('data-') || k === 'role') domProps[k] = v;
        }
    }
    // be3bd132: an editable Para carries the pool id plus tabIndex=-1 so the
    // keyboard-a11y / layout-stable edit wrapper can focus it.
    if (isEditable && poolId !== undefined) {
        domProps['data-block-pool-id'] = poolId;
        domProps.tabIndex = -1;
    }

    // Head of a loose task item: the writer renders `<p><label><input…/>…</label></p>`;
    // the label goes INSIDE the <p> so block-level wrappers stay outside it
    // (bd-qif9l4cx).
    const body = task ? stripTaskMarker(args.node.c) : null;
    const children = task && body ? (
        <TaskLabel state={task}>
            {renderChildren({ ...args, node: { ...args.node, c: body } })}
        </TaskLabel>
    ) : (
        renderChildren(args)
    );

    return <p {...domProps} {...dataLocProps(args.node)}>{children}</p>;
};

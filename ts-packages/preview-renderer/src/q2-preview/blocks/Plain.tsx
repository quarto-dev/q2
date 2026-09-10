import { renderChildren } from '../../framework';
import type { NodeArgs, PlainBlock } from '../../framework';
import { stripTaskMarker, TaskLabel, useTaskItem } from './taskList';

/** Plain renders no wrapper element — its inlines flow into the
 * surrounding context (table cells, list items, etc.).
 *
 * As the head block of a task item (`TaskItemContext` set by the `<li>`,
 * inlines starting with the ballot-box marker) it renders the writer's
 * `<label><input type="checkbox"…/>…</label>` itself, so any block-level
 * wrapper the dispatcher adds stays outside the label (bd-qif9l4cx). */
export const Plain = (args: NodeArgs<PlainBlock>) => {
    const task = useTaskItem();
    const body = task ? stripTaskMarker(args.node.c) : null;
    if (task && body) {
        return (
            <TaskLabel state={task}>
                {renderChildren({ ...args, node: { ...args.node, c: body } })}
            </TaskLabel>
        );
    }
    return <>{renderChildren(args)}</>;
};

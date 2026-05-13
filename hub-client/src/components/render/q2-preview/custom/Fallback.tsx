import { renderChildren } from '@quarto/preview-renderer/framework';
import type {
    CustomBlockNode,
    CustomInlineNode,
    NodeArgs,
} from '@quarto/preview-renderer/framework';

/**
 * Generic CustomNode renderer for unknown / not-yet-implemented
 * `type_name` values. Registered under `previewRegistry['__fallback__']`
 * and dispatched to by `dispatchers.tsx`'s `CustomBlock` / `CustomInline`
 * when the registry has no entry for `node.type_name`.
 *
 * Behavior:
 *   - Display the type_name in a styled wrapper so it's obvious to
 *     the author that the node was reached but not specifically
 *     handled.
 *   - Recurse into all slots via `renderChildren`, which routes
 *     through the framework's `renderCustomNodeChildren` walk
 *     (`framework/dispatch.tsx:238-310`). Every slot child mounts
 *     through `<Node>` so the atomic gate fires correctly.
 *
 * Until Plan 8 ships its IncludeExpansion component, an
 * `IncludeExpansion` wrapper hits this fallback — visually nondescript,
 * but not broken.
 */
const fallbackStyle: React.CSSProperties = {
    border: '1px dashed #aaa',
    padding: '0.5em',
    margin: '0.25em 0',
};

const fallbackBadgeStyle: React.CSSProperties = {
    fontSize: '0.85em',
    color: '#666',
    fontFamily: 'monospace',
};

export const Fallback = (args: NodeArgs<CustomBlockNode | CustomInlineNode>) => {
    const isBlock = args.node.t === 'CustomBlock';
    const Wrapper: React.ElementType = isBlock ? 'div' : 'span';
    return (
        <Wrapper style={fallbackStyle}>
            <span style={fallbackBadgeStyle}>{args.node.type_name}</span>
            {renderChildren(args)}
        </Wrapper>
    );
};

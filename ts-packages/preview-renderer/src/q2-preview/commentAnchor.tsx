/**
 * How the comment chrome finds a block's DOM element (bd-q2wqj24c).
 *
 * `CommentBlock` renders every block UNTOUCHED — no wrapper element —
 * because the theme CSS is written for the native writer's DOM, where a
 * block is a direct child of its container (`blockquote > h4`,
 * `.callout-body > :first-child`, `li > p:last-of-type`, …). Its bubble
 * lives in a body-level overlay layer and is positioned from the
 * block's measured rect, so the chrome needs a handle on the block's
 * host element. Two contracts provide it:
 *
 *  - **`CommentAnchorContext`** — provided by the `CommentBlock` around
 *    the block it renders. A block component whose host element should
 *    anchor the bubble calls `useCommentAnchorRef(args.node)` and spreads
 *    the result as `ref` on that element. The hook hands back the
 *    registration callback only when the context's node IS the
 *    component's node (identity), so a block nested inside a
 *    comment-container Div — or any other nesting — never registers as
 *    its ancestor's anchor. The chrome-eligible components (`Para`,
 *    `Header`, `CodeBlock`, `MermaidCodeBlock`, `Div`) adopt it; a user
 *    `render-components` override that does not is simply chrome-less.
 *
 *  - **`PlainHostContext`** — `Plain` renders a fragment (its inlines go
 *    straight into the parent element: a tight list's `<li>`, a
 *    definition's `<dd>`), so it has no element of its own. The
 *    component that owns that parent element renders it through
 *    `PlainHost`, which provides the element to the `Plain`'s
 *    `CommentBlock`. A `Plain` with no host in scope renders passthrough
 *    (comment spans stay visible in the text) rather than stripping the
 *    comment and showing no bubble.
 */
import React from 'react';

export interface CommentAnchorTarget {
    /** The block node whose host element should register. */
    node: unknown;
    /** Ref callback for that element. */
    register: React.RefCallback<Element>;
}

export const CommentAnchorContext = React.createContext<CommentAnchorTarget | null>(null);

/**
 * The `ref` a block component spreads onto its host element, or
 * `undefined` when no enclosing `CommentBlock` is asking for THIS node.
 */
export function useCommentAnchorRef(node: unknown): React.RefCallback<Element> | undefined {
    const target = React.useContext(CommentAnchorContext);
    if (!target || target.node !== node) return undefined;
    return target.register;
}

export const PlainHostContext = React.createContext<React.RefObject<HTMLElement | null> | null>(null);

type PlainHostProps = {
    /** The element to render — the one a contained `Plain`'s inlines land in. */
    as: 'li' | 'dd';
    children?: React.ReactNode;
} & Record<string, unknown>;

/**
 * Renders `as` with the given props and provides the resulting element
 * to `Plain` blocks rendered inside it (see `PlainHostContext`).
 */
export function PlainHost({ as, children, ...props }: PlainHostProps) {
    const ref = React.useRef<HTMLElement | null>(null);
    return (
        <PlainHostContext.Provider value={ref}>
            {React.createElement(as, { ...props, ref }, children)}
        </PlainHostContext.Provider>
    );
}

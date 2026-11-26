import { useContext, type ReactNode } from 'react';
import { renderChildren, dataLocProps } from '../../framework';
import type { DivBlock, NodeArgs } from '../../framework';
import { IncrementalContext } from '../IncrementalContext';
import { PreviewContext } from '../PreviewContext';
import { useCommentAnchorRef } from '../commentAnchor';
import { ASIDE, NOTES, SECTION } from '../quartoClasses';
import { cssStringToObject } from '../utils';

export const Div = (args: NodeArgs<DivBlock>) => {
    const ctx = useContext(PreviewContext);
    const anchorRef = useCommentAnchorRef(args.node);
    const poolId = (args.node as any).s as string | number | undefined;

    const [[id, classes, kvs]] = args.node.c;
    const props: Record<string, unknown> = {};
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    for (const [k, v] of kvs) {
        if (k.startsWith('data-') || k === 'role') props[k] = v;
    }
    // Inline `style` from the AST (e.g. column `flex-basis` set by
    // RevealColumnsTransform) — the native writer emits it, so the preview
    // must too for layout parity. React needs a style *object*, not a string.
    const styleStr = kvs.find(([k]) => k === 'style')?.[1];
    if (styleStr) props.style = cssStringToObject(styleStr);

    // Incremental scope (revealjs): `.incremental` / `.nonincremental` flip the
    // state for this subtree; speaker notes are never incremental. Mirrors the
    // native writer's `writerIncremental` threading. No-op when not in a deck
    // (IncrementalContext.enabled === false).
    const parentIncremental = useContext(IncrementalContext);
    let childIncremental = parentIncremental.incremental;
    if (classes.includes('incremental')) childIncremental = true;
    else if (classes.includes('nonincremental')) childIncremental = false;
    if (classes.includes(NOTES)) childIncremental = false;
    const wrap = (children: ReactNode): ReactNode =>
        childIncremental === parentIncremental.incremental ? (
            children
        ) : (
            <IncrementalContext.Provider
                value={{ enabled: parentIncremental.enabled, incremental: childIncremental }}
            >
                {children}
            </IncrementalContext.Provider>
        );

    // bd-coffj: mirror the native HTML writer
    // (`crates/pampa/src/writers/html.rs::Block::Div`) — a Pandoc Div
    // whose class list contains "section" (output of the sectionize
    // transform) renders as `<section>`, not `<div>`. Sections are
    // not editable at this phase (Plan 4); non-section divs get the
    // affordance attribute. Quarto theme CSS keys off the `<section>`
    // tag, so emitting `<div>` here would cause spacing drift between
    // `q2 render` and `q2 preview`.
    const locProps = dataLocProps(args.node);
    if (classes.includes(SECTION)) {
        return <section ref={anchorRef} {...props} {...locProps}>{wrap(renderChildren(args))}</section>;
    }
    // Revealjs speaker notes / asides — mirror the native writer's
    // `.notes` / `.aside` → <aside>. reveal.css hides `aside.notes`;
    // quarto-reveal.css styles `aside.aside` (bottom of slide, muted).
    if (classes.includes(NOTES) || classes.includes(ASIDE)) {
        return <aside ref={anchorRef} {...props} {...locProps}>{wrap(renderChildren(args))}</aside>;
    }
    // Code folding (`code-fold`). The native writer gets the fold as a pair
    // of raw `<details>` / `</details>` blocks; React cannot enclose content
    // that way, so `CodeBlockRenderTransform` sends the preview one wrapper
    // Div and this renders the same element.
    if (classes.includes('code-fold')) {
        const summary = kvs.find(([k]) => k === 'data-code-summary')?.[1];
        if (summary !== undefined) {
            const open = kvs.some(([k]) => k === 'data-code-fold-open');
            return (
                <details ref={anchorRef} className={classes.join(' ')} open={open} {...locProps}>
                    <summary>{summary}</summary>
                    {wrap(renderChildren(args))}
                </details>
            );
        }
    }
    // Code annotations: the list under an annotated block. Same reason as
    // the fold: `code_annotations.rs` sends Divs where the native writer
    // gets raw `<dl>` / `<dt>` / `<dd>`.
    if (classes.includes('code-annotation-container-grid')) {
        return (
            <dl ref={anchorRef} className={classes.join(' ')} {...locProps}>
                {wrap(renderChildren(args))}
            </dl>
        );
    }
    if (classes.includes('code-annotation-item')) {
        const cell = kvs.find(([k]) => k === 'data-target-cell')?.[1];
        const annotation = kvs.find(([k]) => k === 'data-target-annotation')?.[1];
        return (
            <>
                <dt data-target-cell={cell} data-target-annotation={annotation}>{annotation}</dt>
                <dd>{wrap(renderChildren(args))}</dd>
            </>
        );
    }
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;
    // INTERIM (RIU-3b): the record splice cannot name its own nodes, so when an expanded cell
    // shifts the block count, reconcile pairs the author's next Div with a spliced `.cell` Div
    // and that Div keeps the author's source info (it resolves, and it is not stamped). Refuse
    // to edit any `.cell` Div rather than rewrite the author's bytes from output. Engines
    // should return provenance for the nodes they produce; then replace this rule and its
    // `interim_` tests (record-output-stamp.integration.test.tsx).
    const isInterimCell = classes.includes('cell');
    const isEditable =
        resolved != null &&
        resolved.reachabilityClass !== 'Opaque' &&
        poolId !== undefined &&
        !isInterimCell;
    if (isEditable) {
        props['data-block-pool-id'] = String(poolId);
        props.tabIndex = -1;
    }
    return <div ref={anchorRef} {...props} {...locProps}>{wrap(renderChildren(args))}</div>;
};

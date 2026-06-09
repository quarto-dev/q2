import { useContext, type CSSProperties, type ReactNode } from 'react';
import { renderChildren } from '../../framework';
import type { DivBlock, NodeArgs } from '../../framework';
import { IncrementalContext } from '../IncrementalContext';
import { PreviewContext } from '../PreviewContext';
import { ASIDE, NOTES, SECTION } from '../quartoClasses';

/**
 * Convert a CSS declaration string (`"flex-basis: 40%; color: red"`) to the
 * React style object React requires. Property names are camelCased; custom
 * properties (`--foo`) are left as-is.
 */
function cssStringToObject(css: string): CSSProperties {
    const style: Record<string, string> = {};
    for (const decl of css.split(';')) {
        const idx = decl.indexOf(':');
        if (idx === -1) continue;
        const prop = decl.slice(0, idx).trim();
        const value = decl.slice(idx + 1).trim();
        if (!prop || !value) continue;
        const key = prop.startsWith('--')
            ? prop
            : prop.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
        style[key] = value;
    }
    return style as CSSProperties;
}

export const Div = (args: NodeArgs<DivBlock>) => {
    const ctx = useContext(PreviewContext);
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
    // affordance attribute.
    if (classes.includes(SECTION)) {
        return <section {...props}>{wrap(renderChildren(args))}</section>;
    }
    // Revealjs speaker notes / asides — mirror the native writer's
    // `.notes` / `.aside` → <aside>. reveal.css hides `aside.notes`;
    // quarto-reveal.css styles `aside.aside` (bottom of slide, muted).
    if (classes.includes(NOTES) || classes.includes(ASIDE)) {
        return <aside {...props}>{wrap(renderChildren(args))}</aside>;
    }
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;
    const isEditable = resolved?.reachabilityClass === 'TopLevel' && poolId !== undefined;
    if (isEditable) props['data-block-pool-id'] = String(poolId);
    return <div {...props}>{wrap(renderChildren(args))}</div>;
};

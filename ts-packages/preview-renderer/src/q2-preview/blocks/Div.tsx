import { renderChildren } from '../../framework';
import type { DivBlock, NodeArgs } from '../../framework';
import { NOTES, SECTION } from '../quartoClasses';

export const Div = (args: NodeArgs<DivBlock>) => {
    const [[id, classes, kvs]] = args.node.c;
    const props: Record<string, string> = {};
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    for (const [k, v] of kvs) {
        if (k.startsWith('data-') || k === 'role') props[k] = v;
    }
    // bd-coffj: mirror the native HTML writer
    // (`crates/pampa/src/writers/html.rs::Block::Div`) — a Pandoc Div
    // whose class list contains "section" (output of the sectionize
    // transform) renders as `<section>`, not `<div>`. Quarto theme
    // CSS keys off the `<section>` tag (e.g.
    // `main.content > p:has(+ section) { margin-bottom: 2rem }`), so
    // emitting `<div>` here causes visible spacing drift between
    // `q2 render` and `q2 preview`.
    if (classes.includes(SECTION)) {
        return <section {...props}>{renderChildren(args)}</section>;
    }
    // Revealjs speaker notes — mirror the native writer's `.notes` → <aside>
    // so `q2 preview` and `q2 render` agree (reveal.css hides `aside.notes`).
    if (classes.includes(NOTES)) {
        return <aside {...props}>{renderChildren(args)}</aside>;
    }
    return <div {...props}>{renderChildren(args)}</div>;
};

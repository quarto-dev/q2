/**
 * Shared pieces of the dashboard preview (`format: dashboard` previews as
 * the `q2-dashboard` pseudo-format; plan
 * `claude-notes/plans/2026-11-03-dashboard-format-port.md`, Phase 8).
 *
 * The Rust lowering (`crates/quarto-core/src/dashboard/resolve.rs`) builds
 * the bslib markup for render and preview alike; in the preview,
 * `dashboard/view.rs` turns the elements whose behavior Q1 left to bslib's
 * and Bootstrap's JavaScript into view nodes. Each view node carries the
 * element's attributes, exactly as the render writes them, in `node.attr`,
 * so these components only add behavior (active page and tab, sidebar
 * collapse, card full screen) as React state.
 */
import { createContext, type CSSProperties } from 'react';
import type { Attr } from '../../../framework';
import { cssStringToObject } from '../../utils';

/** Classes the components own: they are set from React state, not from the
 *  render's initial markup. */
const STATE_CLASSES = new Set(['active', 'show']);

/**
 * React props for an element from its Pandoc attr: id, class, style,
 * `data-*`, `aria-*` and `role` (the attributes the lowering writes).
 */
export function attrProps(attr: Attr): Record<string, unknown> {
    const [id, classes, kvs] = attr;
    const props: Record<string, unknown> = {};
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    for (const [k, v] of kvs) {
        if (k === 'style') props.style = cssStringToObject(v) as CSSProperties;
        else if (k.startsWith('data-') || k.startsWith('aria-') || k === 'role') props[k] = v;
    }
    return props;
}

/** `className` plus extra classes. */
export function withClasses(props: Record<string, unknown>, ...extra: (string | false)[]): string {
    return [props.className as string | undefined, ...extra].filter(Boolean).join(' ');
}

/**
 * The element's classes with `active` / `show` set from state: kept where
 * the render put them when on (so the initial preview matches the render
 * class for class), appended when newly on, dropped when off.
 */
export function stateClassName(classes: string[], on: boolean): string {
    if (!on) return classes.filter((c) => !STATE_CLASSES.has(c)).join(' ');
    const out = classes.slice();
    for (const c of ['show', 'active']) if (!out.includes(c)) out.push(c);
    return out.join(' ');
}

/** The active dashboard page (the navbar's page tabs). */
export type DashboardPages = {
    active: string | null;
    setActive: (id: string) => void;
};
export const DashboardPagesContext = createContext<DashboardPages>({
    active: null,
    setActive: () => {},
});

/** A tabset card's active tab (its nav and its panes). */
export type CardTabs = {
    active: string | null;
    setActive: (id: string) => void;
};
export const CardTabsContext = createContext<CardTabs | null>(null);

/** A tab pane is active when chosen, or by default when it is the first
 *  (`<tabset-id>-1`, as the render marks it). */
export function isActivePane(id: string, active: string | null): boolean {
    return active === null ? /-1$/.test(id) : active === id;
}

/**
 * Chrome HTML-injection slots for q2-preview's `PreviewDocument`
 * (Phase F.2, bd-kw93.15).
 *
 * Each slot reads an HTML string from `meta.rendered.navigation.*`
 * (populated by the corresponding `*-render` transform on the Rust
 * side) and injects it via `dangerouslySetInnerHTML`. The slots
 * are wrapped in `React.memo` keyed on the HTML string, so an
 * identical re-post (e.g. an edit to body content that doesn't
 * change the chrome) doesn't tear down the chrome DOM.
 *
 * The DOM-stability compromise of `dangerouslySetInnerHTML` is the
 * known trade-off F.2 accepts: when the chrome HTML *does* change
 * (a `_quarto.yml` edit, a page switch that updates the active
 * sidebar entry), Bootstrap's open dropdowns / collapse states get
 * reset because the inner DOM is replaced wholesale. bd-d8fo
 * tracks the React-components rewrite that fixes this.
 *
 * Wrapper structure mirrors `crates/quarto-core/src/template.rs`
 * lines 178-254 byte-for-byte for what's *outside* the injected
 * HTML; the injected HTML itself is what each `*-render` transform
 * produces (so it stays byte-identical with `q2 render`).
 *
 * The favicon and other `<head>` includes don't have a slot here —
 * they're handled by [`HeaderIncludesEffect`] below, which
 * imperatively manages `<link>` / `<meta>` elements in
 * `document.head` (no React-level rendering for elements that
 * don't live in the body).
 */

import { memo, useEffect } from 'react';

interface SlotProps {
    html: string;
}

/**
 * Navbar HTML — emitted by `NavbarRenderTransform` into
 * `meta.rendered.navigation.navbar`. Renders BEFORE
 * `<div id="quarto-content">` (per template.rs:178-180).
 */
export const NavbarSlot = memo(({ html }: SlotProps) => (
    <div dangerouslySetInnerHTML={{ __html: html }} />
));
NavbarSlot.displayName = 'NavbarSlot';

/**
 * Sidebar HTML — emitted by `SidebarRenderTransform` into
 * `meta.rendered.navigation.sidebar`. Renders INSIDE
 * `<div id="quarto-content">`, BEFORE the TOC and `<main>`
 * (per template.rs:186-188).
 */
export const SidebarSlot = memo(({ html }: SlotProps) => (
    <div dangerouslySetInnerHTML={{ __html: html }} />
));
SidebarSlot.displayName = 'SidebarSlot';

/**
 * Page-navigation (prev/next) HTML — emitted by
 * `PageNavRenderTransform` into
 * `meta.rendered.navigation.page_navigation`. Renders INSIDE
 * `<main>`, AFTER the body content (per template.rs:244-246).
 */
export const PageNavSlot = memo(({ html }: SlotProps) => (
    <div dangerouslySetInnerHTML={{ __html: html }} />
));
PageNavSlot.displayName = 'PageNavSlot';

/**
 * Page-footer HTML — emitted by `FooterRenderTransform` into
 * `meta.rendered.navigation.footer`. Renders AFTER
 * `<div id="quarto-content">` (per template.rs:252-254).
 */
export const FooterSlot = memo(({ html }: SlotProps) => (
    <div dangerouslySetInnerHTML={{ __html: html }} />
));
FooterSlot.displayName = 'FooterSlot';

/**
 * TOC HTML (the inner `<ul>`) — emitted by `TocRenderTransform`
 * into `meta.rendered.navigation.toc`. The wrapping
 * `<div id="quarto-margin-sidebar"><nav id="TOC"><h2>...</h2>`
 * is supplied by the template (template.rs:189-200), so this slot
 * only fills the inner `<ul>`. Memo keyed on (html, title) so a
 * `toc-title:` edit re-renders correctly without tearing the DOM
 * for content edits.
 */
export const TocSlot = memo(({ html, title }: SlotProps & { title: string }) => (
    <div
        id="quarto-margin-sidebar"
        className="sidebar margin-sidebar"
    >
        <nav id="TOC" role="doc-toc" className="toc-active">
            {title && <h2 id="toc-title">{title}</h2>}
            <div dangerouslySetInnerHTML={{ __html: html }} />
        </nav>
    </div>
));
TocSlot.displayName = 'TocSlot';

interface HeaderIncludesProps {
    /**
     * Raw HTML strings to append to `document.head`. Each entry is
     * parsed and its top-level children are inserted with a
     * `data-q2-header-include` marker for the cleanup pass to find.
     * Identity comparison via the joined string in the dep array;
     * pass already-derived list shape (callers use
     * `extractMetaStringList` upstream).
     */
    items: readonly string[];
}

/**
 * Imperative `<head>` injector for `meta.rendered.includes.header`
 * (favicon `<link>`, listing-feed `<link rel="alternate">`, etc.).
 * Not a render-time slot because elements like `<meta>` and `<link>`
 * don't belong in `<body>` — they have to land in `document.head`,
 * which React doesn't own.
 *
 * Cleanup on unmount removes the previously-inserted nodes so
 * test re-mounts (vitest, Playwright) don't accumulate state.
 *
 * Idempotent on re-mount: each effect run replaces the previous set
 * via cleanup, so re-rendering the same items does not duplicate.
 */
export function HeaderIncludesEffect({ items }: HeaderIncludesProps) {
    // Joined string is the dep so the effect only re-runs when the
    // bag of include strings actually changes. Per-item identity
    // doesn't matter; the markers + cleanup handle the diff.
    const key = items.join('\0');
    useEffect(() => {
        if (items.length === 0) return;
        const wrapper = document.createElement('div');
        wrapper.innerHTML = items.join('\n');
        const inserted: Element[] = [];
        for (const el of Array.from(wrapper.children)) {
            el.setAttribute('data-q2-header-include', '1');
            document.head.appendChild(el);
            inserted.push(el);
        }
        return () => {
            for (const el of inserted) {
                el.remove();
            }
        };
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [key]);
    return null;
}

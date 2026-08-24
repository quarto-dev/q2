import { useContext, useEffect } from 'react';
import {
    renderChildren,
    extractMetaString,
    extractMetaBool,
    extractMetaStringList,
    getMetaPath,
    RegistryContext,
    useAttributionHover,
} from '../framework';
import type { BlockNode, PandocAST } from '../framework';
import * as Custom from './custom';
import {
    NavbarSlot,
    SecondaryNavSlot,
    SidebarSlot,
    PageNavSlot,
    FooterSlot,
    TocSlot,
    HeaderIncludesEffect,
} from './chromeSlots';
import { useBlockEditHover } from './useBlockEditHover';

/**
 * bd-elgxx (D6 react): mirror of Rust
 * `crates/quarto-core/src/template.rs::append_color_mode_class`.
 *
 * Appends `quarto-light` to the structural body-class string so
 * theme-conditional CSS keys off `body.quarto-light` (matches Q1's
 * default body class). Idempotent: if either `quarto-light` or
 * `quarto-dark` is already in the structural classes, the input is
 * returned unchanged (the user / a future dark-mode resolver wins).
 *
 * Light/dark theme detection is not yet wired into the pipeline; when
 * it lands, this helper grows a `mode` argument and the caller
 * decides which class to emit. The Rust helper has the same shape and
 * the same TODO.
 */
function appendColorModeClass(structural: string): string {
    const LIGHT = 'quarto-light';
    const already = structural
        .split(/\s+/)
        .some((tok) => tok === LIGHT || tok === 'quarto-dark');
    if (already) return structural;
    if (structural === '') return LIGHT;
    return `${structural} ${LIGHT}`;
}

/**
 * q2-preview's document-root wrapper. Registered into `registry.ts`
 * under the `'Ast'` key. Mirrors the HTML pipeline's
 * `<div id="quarto-content"><main class="content">` wrapper at
 * `crates/quarto-core/src/template.rs:185-247` byte-for-byte.
 *
 * `body-classes` is applied imperatively to `document.body` in a
 * `useEffect`; the wrapper itself emits the inner `<div>` + `<main>`.
 * `minimal: true` and `theme: none | pandoc` skip the wrapper entirely
 * (mirrors Rust's `is_minimal_html()` at `format.rs:306-318`).
 *
 * Minimal-mode title synthesis (Phase 6.2): the Rust `title-block`
 * transform is in `Q2_PREVIEW_TRANSFORM_EXCLUDED`
 * (`pipeline.rs:1052`), so without React-side replication q2-preview's
 * minimal mode silently drops the title. When the document is
 * minimal AND has a `title` AND no user-authored level-1 Header,
 * prepend a synthetic `<h1>{title}</h1>`.
 *
 * Iframe `<title>` (Phase 6.2a): when `meta.pagetitle` or `meta.title`
 * resolves to a non-empty string, write `document.title`; restore on
 * unmount. When neither resolves, leave the static title from
 * `q2-preview.html:6` alone.
 */
export const PreviewDocument = ({
    ast,
    onNavigateToDocument,
    setAst,
}: {
    ast: PandocAST;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
}) => {
    const meta = ast.meta ?? {};

    // Mirror Rust template.rs:415-417: page-layout defaults to "article".
    const pageLayout = extractMetaString(meta['page-layout']) ?? 'article';

    // Chrome HTML needed by both the body-class compose (below) and
    // the header wrapper (in the JSX): hoisted here so the two agree.
    const navbarHtml = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'navbar']),
    );
    const secondaryNavHtml = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'secondary-nav']),
    );

    // Mirror Rust `render_with_compiled_template`'s body-class
    // compose (the accumulating list, bd-ersobfbt). Precedence for the
    // structural base:
    //   1. User-set top-level `body-classes` (wins WHOLESALE — the
    //      compose below, including the nav-fixed append, is skipped,
    //      matching the Rust `ctx.get("body-classes").is_none()` guard).
    //   2. `rendered.navigation.body-classes` populated by
    //      `SidebarRenderTransform` (e.g. `nav-sidebar floating`,
    //      `nav-sidebar docked`) — required for Bootstrap's
    //      sidebar-aware grid layout to take effect.
    //   3. TOC rendered but no sidebar → empty class. Falls through
    //      to the default (no-class) wide grid, whose right margin
    //      column is `minmax(0.3*mw, 0.58*mw)` and has room for the
    //      TOC. The `fullcontent` mixin's margin column is only
    //      `0.28*mw` (~70px at the default 250px) and squashes a TOC.
    //   4. Otherwise → literal `fullcontent`.
    // Onto the structural base: `nav-fixed` when a navbar is rendered
    // (bd-ersobfbt — the `#quarto-header` is `fixed-top`, and
    // `body.nav-fixed { padding-top }` is its pre-JS compensation;
    // same condition as Q1's postprocessor and the Rust compose in
    // template.rs — keep the three in sync). A secondary-nav-only
    // page (sidebar site, no navbar) gets NO nav-fixed, matching Q1.
    // Empty string for the *structural* class is the user's opt-out;
    // only `undefined` triggers the fallback chain. The color-mode
    // class (`quarto-light`) is then appended in all cases (bd-tkamn).
    const userBodyClasses = extractMetaString(meta['body-classes']);
    const renderedBodyClasses = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'body-classes']),
    );
    const tocRendered = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'toc']),
    );
    const hasToc = tocRendered !== undefined && tocRendered !== '';
    const structuralBodyClasses =
        userBodyClasses ??
        renderedBodyClasses ??
        (hasToc ? '' : 'fullcontent');
    const composedBodyClasses =
        userBodyClasses === undefined && navbarHtml
            ? [structuralBodyClasses, 'nav-fixed']
                  .filter((c) => c !== '')
                  .join(' ')
            : structuralBodyClasses;
    // bd-tkamn (D6 react): mirror Rust `template.rs::append_color_mode_class`
    // so the preview's `<body>` carries `quarto-light` regardless of
    // how the structural class was computed (including the TOC-
    // present empty-structural case). Theme-conditional CSS can then
    // key off `body.quarto-light`. Dark-mode support is deferred
    // until the pipeline grows light/dark theme configs; until then
    // the append is unconditional (matches Q1 default).
    const bodyClasses = appendColorModeClass(composedBodyClasses);

    // Mirror Rust is_minimal_html() (format.rs:306-318).
    const minimal =
        extractMetaBool(meta.minimal) === true ||
        extractMetaString(meta.theme) === 'none' ||
        extractMetaString(meta.theme) === 'pandoc';

    // Resolve the title-block component via the registry so user TSX
    // can override it under the synthetic `__title_block__` key.
    // Pattern matches the dispatchers at `dispatchers.tsx:39`. Called
    // unconditionally before any early returns to keep hook order
    // stable (React rules-of-hooks).
    const { registry } = useContext(RegistryContext);
    const TitleBlock = registry.__title_block__ ?? Custom.PreviewTitleBlock;

    // Imperative body-class management. Restore on unmount so test
    // re-mounts (vitest, Playwright) start with a clean slate.
    useEffect(() => {
        const previous = document.body.className;
        document.body.className = bodyClasses;
        return () => {
            document.body.className = previous;
        };
    }, [bodyClasses]);

    // Iframe `<title>` wiring. Writes only when an AST title resolves
    // to a non-empty string. Cleanup restores the pre-mount value
    // (which equals the static literal from q2-preview.html:6 on
    // first mount).
    useEffect(() => {
        const next =
            extractMetaString(meta.pagetitle) ??
            extractMetaString(meta.title);
        if (!next) return;
        const previous = document.title;
        document.title = next;
        return () => {
            document.title = previous;
        };
    }, [meta]);

    // Phase F.2 (bd-kw93.15): chrome HTML strings populated by the
    // `*-render` transforms now in the q2-preview pipeline. Each
    // slot is React.memo'd so an identical re-post (edit to body
    // content) doesn't tear down the chrome DOM. (navbarHtml and
    // secondaryNavHtml are hoisted above the body-class compose.)
    const sidebarHtml = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'sidebar']),
    );
    const pageNavHtml = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'page_navigation']),
    );
    const tocHtml = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'toc']),
    );
    const footerHtml = extractMetaString(
        getMetaPath(meta, ['rendered', 'navigation', 'footer']),
    );
    // TocGenerateTransform always sets the TOC title (default
    // "Table of Contents" — toc_generate.rs), but a user can still
    // override it or set it to empty; the slot omits the `<h2>` when
    // the title is empty.
    //
    // Read the *rendered* value, not `navigation.toc.title`: the title
    // carries inline markup (`toc-title: "On **this** page"`), so the
    // metadata value is `PandocInlines`, which `extractMetaString`
    // cannot see. `TocRenderTransform` renders it to HTML alongside the
    // entries, and `q2 render`'s template reads the same key — which is
    // what keeps preview and render in agreement
    // (bd-toc-smart-quotes-6nro57ed).
    const tocTitleHtml =
        extractMetaString(
            getMetaPath(meta, ['rendered', 'navigation', 'toc-title']),
        ) ?? '';
    // `meta.rendered.includes.header` collects favicon / RSS links /
    // any user includes-in-header (already a list; q2 render's
    // template wires `$header-includes$` into `<head>`). The banner
    // mode's generated <style> arrives through the same list.
    const headerIncludes = extractMetaStringList(
        getMetaPath(meta, ['rendered', 'includes', 'header']),
    );
    // Banner mode flag (P5, bd-364ol5lu), written by the pipeline's
    // `TitleBannerTransform`: hoists the title block above
    // #quarto-content and adds `quarto-banner-title-block` to <main>.
    const bannerTitleBlock =
        extractMetaBool(
            getMetaPath(meta, ['rendered', 'title-block-banner']),
        ) === true;

    // Attribution wiring (Phase 3 of
    // `2026-05-13-q2-preview-attribution.md`): delegated to
    // `useAttributionHover`, which returns inert wiring when
    // `AttributionLookupContext` is unpopulated. The q2-preview
    // render path is currently in that state — `render_page_for_preview`
    // does not yet install a `PreBuiltAttributionProvider`, so
    // `astContext.attribution*` arrives empty, the context is empty,
    // and every interpolation below is a no-op (DOM byte-identical
    // to pre-attribution). When the producer-side gap is closed the
    // consumer wiring lights up automatically with no React changes.
    const attr = useAttributionHover();
    const blockEdit = useBlockEditHover();

    const children = renderChildren({
        node: ast as any,
        setLocalAst: setAst as any,
        onNavigateToDocument,
    });

    if (minimal) {
        // Re-implement the Rust title-block transform's minimal-mode
        // branch on the React side. Rust's transform (excluded from
        // q2-preview's pipeline; behavior at
        // transforms/title_block.rs:54-110) prepends a synthetic
        // level-1 Header from meta.title when minimal AND no
        // user-authored level-1 Header exists.
        const title = extractMetaString(meta.title);
        const hasLevel1Header = (ast.blocks ?? []).some(
            (b: BlockNode) =>
                (b as { t?: string }).t === 'Header' &&
                Array.isArray((b as { c?: unknown[] }).c) &&
                ((b as { c: unknown[] }).c[0] === 1),
        );
        const minimalInner = (
            <>
                {title && !hasLevel1Header ? <h1>{title}</h1> : null}
                {children}
            </>
        );
        // When attribution is on we need a host element to carry the
        // mouseover delegation. Off-path stay on the Fragment so the
        // minimal-mode DOM is byte-identical to today's.
        if (attr.enabled) {
            return (
                <>
                    {attr.stylesheet}
                    {blockEdit.stylesheet}
                    <div {...attr.hostProps} {...blockEdit.hostProps}>{minimalInner}</div>
                    {attr.overlay}
                </>
            );
        }
        return minimalInner;
    }

    return (
        <>
            {/* Attribution stylesheet — inert (renders nothing) when
                attribution context is empty. Lives next to header-
                includes since both are document-head-adjacent. */}
            {attr.stylesheet}
            {/* Block-edit hover outline CSS (Plan 2b). */}
            {blockEdit.stylesheet}

            {/* Phase F.2: header-includes (favicon, RSS links, user
                includes) appended imperatively to `document.head`. */}
            <HeaderIncludesEffect items={headerIncludes} />

            {/* The site header lives BEFORE quarto-content, wrapping
                the navbar and the narrow-viewport secondary nav
                (template.rs QUARTO_HEADER_PARTIAL + its call-site gate).
                The <header> element is React-owned — NOT part of the
                dangerouslySetInnerHTML chrome — so a live Headroom
                instance (entry.tsx injects headroom.min.js +
                quarto-nav.js) stays bound to a stable element while the
                chrome HTML inside it churns (bd-ersobfbt, decision 8).
                Classes mirror the template: `headroom fixed-top`
                unconditionally (Q1 parity — also under pinned:), plus
                `quarto-banner` in banner mode. */}
            {navbarHtml || secondaryNavHtml ? (
                <header
                    id="quarto-header"
                    className={`headroom fixed-top${bannerTitleBlock ? ' quarto-banner' : ''}`}
                    // Preview analogue of the native `pinned:` opt-out
                    // (transforms/quarto_nav_js.rs omits headroom.min.js;
                    // the preview bundle always contains it, so the header
                    // is tagged instead and quarto-nav.js declines to bind
                    // Headroom to a tagged header). Same config keys as the
                    // native decide().
                    {...(extractMetaBool(
                        getMetaPath(meta, ['navigation', 'navbar', 'pinned']),
                    ) === true ||
                    extractMetaBool(
                        getMetaPath(meta, ['navigation', 'sidebar', 'pinned']),
                    ) === true
                        ? { 'data-headroom-pinned': 'true' }
                        : {})}
                >
                    {navbarHtml ? <NavbarSlot html={navbarHtml} /> : null}
                    {secondaryNavHtml ? (
                        <SecondaryNavSlot html={secondaryNavHtml} />
                    ) : null}
                </header>
            ) : null}

            {/* Banner mode (P5, bd-364ol5lu): the title block renders
                ABOVE #quarto-content, mirroring FULL_HTML_TEMPLATE's
                $if(rendered.title-block-banner)$ conditional. */}
            {bannerTitleBlock ? (
                <TitleBlock
                    ast={ast}
                    setAst={setAst}
                    onNavigateToDocument={onNavigateToDocument}
                />
            ) : null}

            <div
                id="quarto-content"
                className={`quarto-container page-columns page-rows-contents page-layout-${pageLayout}`}
                {...attr.hostProps}
                {...blockEdit.hostProps}
            >
                {/* Sidebar — INSIDE quarto-content, before TOC + main
                    (template.rs:186-188). */}
                {sidebarHtml ? <SidebarSlot html={sidebarHtml} /> : null}

                {/* TOC — INSIDE quarto-content, before main
                    (template.rs:189-200). */}
                {tocHtml ? <TocSlot html={tocHtml} titleHtml={tocTitleHtml} /> : null}

                <main
                    className={`content${bannerTitleBlock ? ' quarto-banner-title-block' : ''}`}
                    id="quarto-document-content"
                >
                    {!bannerTitleBlock ? (
                        <TitleBlock
                            ast={ast}
                            setAst={setAst}
                            onNavigateToDocument={onNavigateToDocument}
                        />
                    ) : null}
                    {children}

                    {/* Page-nav (prev/next) — INSIDE main, after body
                        content (template.rs:244-246). */}
                    {pageNavHtml ? <PageNavSlot html={pageNavHtml} /> : null}
                </main>
            </div>

            {/* Page-footer lives AFTER quarto-content
                (template.rs:252-254). */}
            {footerHtml ? <FooterSlot html={footerHtml} /> : null}

            {/* Attribution overlay — inert when off-path. */}
            {attr.overlay}
        </>
    );
};

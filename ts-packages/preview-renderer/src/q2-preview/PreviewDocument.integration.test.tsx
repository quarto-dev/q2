/**
 * Vitest tests for `PreviewDocument` body container + iframe `<title>`
 * (Plan 2D Phase 6.3).
 *
 * Mounts via `<Ast>` with `previewRegistry` so the registry's `Ast`
 * entry resolves to `PreviewDocument`. The wrapper structure and
 * body-class / document.title side effects are asserted against the
 * Rust template's structure at `template.rs:185-247`.
 */

import { describe, expect, it, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/react';
import { Ast } from '../framework';
import type { PandocAST } from '../framework';
import { previewRegistry } from './registry';
import { rematerializeScript } from './chromeSlots';

function astJson(meta: Record<string, unknown>, blocks: any[] = []): string {
    const ast: PandocAST = {
        'pandoc-api-version': [1, 23, 0],
        meta,
        blocks: blocks as any,
    };
    return JSON.stringify(ast);
}

function mount(meta: Record<string, unknown>, blocks: any[] = []) {
    return render(
        <Ast
            astJson={astJson(meta, blocks)}
            currentFilePath="/project/test.qmd"
            onNavigateToDocument={() => {}}
            setAst={() => {}}
            registry={previewRegistry}
        />,
    );
}

const STR = (c: string) => ({ t: 'Str', c });
const PARA = (...inlines: any[]) => ({ t: 'Para', c: inlines });
const HEADER = (level: number, text: string) => ({
    t: 'Header',
    c: [level, ['', [], []], [STR(text)]],
});
const ms = (c: string) => ({ t: 'MetaString', c });
const mb = (c: boolean) => ({ t: 'MetaBool', c });
const mm = (entries: Record<string, unknown>) => ({
    t: 'MetaMap',
    c: Object.entries(entries).map(([key, value]) => ({ key, value })),
});

// Snapshot body.className so other tests in the suite aren't observed
// in a polluted state. Vitest happy-dom resets the DOM per file by
// default but we still want a deterministic clean slate per test.
let priorBodyClass: string;
let priorTitle: string;
beforeEach(() => {
    priorBodyClass = document.body.className;
    priorTitle = document.title;
    document.body.className = '';
    document.title = '__test-sentinel__';
});
afterEach(() => {
    document.body.className = priorBodyClass;
    document.title = priorTitle;
});

describe('PreviewDocument body container', () => {
    it('default render: page-layout-article + main.content#quarto-document-content', () => {
        const { container } = mount({}, [PARA(STR('hello'))]);
        const wrapper = container.querySelector('div#quarto-content');
        expect(wrapper).not.toBeNull();
        expect(wrapper!.className).toBe(
            'quarto-container page-columns page-rows-contents page-layout-article',
        );
        const main = wrapper!.querySelector(
            'main.content#quarto-document-content',
        );
        expect(main).not.toBeNull();
        // The paragraph is rendered inside <main>.
        expect(main!.querySelector('p')!.textContent).toBe('hello');
    });

    it('page-layout: full → div.page-layout-full', () => {
        const { container } = mount({ 'page-layout': ms('full') });
        expect(
            container.querySelector('div#quarto-content.page-layout-full'),
        ).not.toBeNull();
    });

    it('page-layout: custom value flows verbatim (no enum validation)', () => {
        const { container } = mount({ 'page-layout': ms('custom') });
        expect(
            container.querySelector('div#quarto-content.page-layout-custom'),
        ).not.toBeNull();
    });

    it('body-classes: custom-cls → document.body.className === "custom-cls quarto-light"', () => {
        // bd-elgxx (D6 react): mirror Rust `append_color_mode_class` —
        // the color-mode class is appended regardless of the
        // structural class source.
        mount({ 'body-classes': ms('custom-cls') });
        expect(document.body.className).toBe('custom-cls quarto-light');
    });

    it('body-classes: "" (empty string) still gets quarto-light', () => {
        // bd-elgxx (D6 react): even when the structural class is empty,
        // the color-mode class survives so theme-conditional CSS keys
        // off `body.quarto-light` (matches the Rust template helper).
        mount({ 'body-classes': ms('') });
        expect(document.body.className).toBe('quarto-light');
    });

    it('default → document.body.className === "fullcontent quarto-light"', () => {
        // bd-elgxx (D6 react): default body class appends `quarto-light`
        // for parity with the Rust template (commit 21c8ec04).
        mount({});
        expect(document.body.className).toBe('fullcontent quarto-light');
    });

    it('TOC rendered but no sidebar → body class is "quarto-light" only (avoids fullcontent squashing TOC)', () => {
        // Mirrors the Rust `render_with_compiled_template` body-class
        // computation: when `rendered.navigation.toc` is non-empty and
        // no sidebar `body-classes` is set, the structural class
        // falls through to the default (no-class) wide grid, whose
        // right-margin column has room for the TOC. The `fullcontent`
        // mixin's margin column is only ~70px at the default and
        // squashes the TOC.
        //
        // bd-tkamn (D6 react): even when the structural class drops
        // to empty, the color-mode class `quarto-light` is still
        // appended — same as the Rust template
        // (`test_full_template_toc_present_yields_empty_body_class`).
        mount({
            rendered: {
                t: 'MetaMap',
                c: [
                    {
                        key: 'navigation',
                        key_source: null,
                        value: {
                            t: 'MetaMap',
                            c: [
                                {
                                    key: 'toc',
                                    key_source: null,
                                    value: { t: 'MetaString', c: '<nav id="TOC"></nav>' },
                                },
                            ],
                        },
                    },
                ],
            },
        });
        expect(document.body.className).toBe('quarto-light');
    });

    it('cleanup: unmount restores the pre-mount body.className', () => {
        document.body.className = 'pre-existing';
        const { unmount } = mount({ 'body-classes': ms('mid') });
        expect(document.body.className).toBe('mid quarto-light');
        unmount();
        expect(document.body.className).toBe('pre-existing');
    });

    it('body-classes already containing quarto-light is not duplicated', () => {
        // bd-elgxx (D6 react): idempotent — a user who explicitly puts
        // `quarto-light` in their body-classes shouldn't end up with two
        // copies. Mirrors `append_color_mode_class` in template.rs.
        mount({ 'body-classes': ms('custom-cls quarto-light') });
        expect(document.body.className).toBe('custom-cls quarto-light');
    });

    it('body-classes containing quarto-dark suppresses the quarto-light append', () => {
        // bd-elgxx (D6 react): when an explicit dark-mode class is
        // already present, do NOT also append light. The Rust helper
        // treats either `quarto-light` or `quarto-dark` as a signal
        // that the color mode is already set.
        mount({ 'body-classes': ms('custom-cls quarto-dark') });
        expect(document.body.className).toBe('custom-cls quarto-dark');
    });
});

describe('PreviewDocument banner placement (P5, bd-364ol5lu)', () => {
    it('banner flag → title block renders BEFORE #quarto-content, main gains quarto-banner-title-block', () => {
        const { container } = mount(
            {
                title: ms('Doc'),
                rendered: mm({
                    'has-title-block': mb(true),
                    'title-block-banner': mb(true),
                }),
            },
            [PARA(STR('hello'))],
        );
        const header = container.querySelector('header#title-block-header');
        const content = container.querySelector('div#quarto-content');
        expect(header).not.toBeNull();
        expect(content).not.toBeNull();
        // The header is a preceding sibling of #quarto-content, not
        // inside it (mirrors FULL_HTML_TEMPLATE's banner conditional).
        expect(content!.contains(header)).toBe(false);
        expect(
            header!.compareDocumentPosition(content!) &
                Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
        expect(
            container.querySelector(
                'main.content.quarto-banner-title-block#quarto-document-content',
            ),
        ).not.toBeNull();
    });

    it('no banner flag → title block stays inside <main>, no banner class', () => {
        const { container } = mount(
            {
                title: ms('Doc'),
                rendered: mm({ 'has-title-block': mb(true) }),
            },
            [PARA(STR('hello'))],
        );
        const main = container.querySelector(
            'main.content#quarto-document-content',
        );
        expect(
            main!.querySelector('header#title-block-header'),
        ).not.toBeNull();
        expect(main!.classList.contains('quarto-banner-title-block')).toBe(
            false,
        );
    });
});

describe('PreviewDocument minimal mode (no wrapper)', () => {
    it('minimal: true → no #quarto-content wrapper, no <main>', () => {
        const { container } = mount({ minimal: mb(true) }, [PARA(STR('x'))]);
        expect(container.querySelector('div#quarto-content')).toBeNull();
        expect(container.querySelector('main.content')).toBeNull();
        // Paragraph still rendered.
        expect(container.querySelector('p')!.textContent).toBe('x');
    });

    it('theme: none → no wrapper', () => {
        const { container } = mount({ theme: ms('none') });
        expect(container.querySelector('div#quarto-content')).toBeNull();
    });

    it('theme: pandoc → no wrapper', () => {
        const { container } = mount({ theme: ms('pandoc') });
        expect(container.querySelector('div#quarto-content')).toBeNull();
    });

    it('minimal: true + title + no level-1 Header → synthetic <h1> before body', () => {
        const { container } = mount(
            { minimal: mb(true), title: ms('Doc Title') },
            [PARA(STR('body'))],
        );
        expect(container.querySelector('div#quarto-content')).toBeNull();
        const h1 = container.querySelector('h1');
        expect(h1).not.toBeNull();
        expect(h1!.textContent).toBe('Doc Title');
        // Synthetic h1 precedes the paragraph in document order.
        const all = Array.from(container.querySelectorAll('h1, p'));
        expect(all[0].tagName).toBe('H1');
        expect(all[1].tagName).toBe('P');
    });

    it('minimal: true + title + user-authored level-1 Header → no synthetic <h1>', () => {
        const { container } = mount(
            { minimal: mb(true), title: ms('Doc Title') },
            [HEADER(1, 'User Heading'), PARA(STR('body'))],
        );
        const h1s = container.querySelectorAll('h1');
        expect(h1s.length).toBe(1);
        expect(h1s[0].textContent).toBe('User Heading');
    });

    it('minimal: true + no title → no synthetic <h1>', () => {
        const { container } = mount({ minimal: mb(true) }, [PARA(STR('x'))]);
        expect(container.querySelector('h1')).toBeNull();
    });
});

describe('PreviewDocument iframe document.title wiring', () => {
    it('writes document.title from meta.title', () => {
        mount({ title: ms('My Doc') });
        expect(document.title).toBe('My Doc');
    });

    it('meta.pagetitle wins over meta.title', () => {
        mount({ pagetitle: ms('Page'), title: ms('Doc') });
        expect(document.title).toBe('Page');
    });

    it('no title and no pagetitle → document.title is unchanged (sentinel preserved)', () => {
        document.title = '__test-sentinel__';
        mount({});
        expect(document.title).toBe('__test-sentinel__');
    });

    it('empty-string title is Pandoc-falsy — no write, sentinel preserved', () => {
        document.title = '__test-sentinel__';
        mount({ title: ms('') });
        expect(document.title).toBe('__test-sentinel__');
    });

    it('cleanup: unmount restores pre-mount document.title', () => {
        document.title = '__before-mount__';
        const { unmount } = mount({ title: ms('Mounted Title') });
        expect(document.title).toBe('Mounted Title');
        unmount();
        expect(document.title).toBe('__before-mount__');
    });
});

// ──────────────────────────────────────────────────────────────────
// Phase F.2 (bd-kw93.15): chrome HTML-injection slots
// ──────────────────────────────────────────────────────────────────

/** Build a `MetaMap` value carrying `entries`. Mirrors the JSON
 *  shape from `crates/pampa/src/writers/json.rs::write_config_value`
 *  (with `key_source: null`). */
function metaMap(entries: Array<{ key: string; value: unknown }>): unknown {
    return {
        t: 'MetaMap',
        c: entries.map((e) => ({ key: e.key, key_source: null, value: e.value })),
    };
}

/** Convenience: shape the `meta.rendered.navigation.<key>: html`
 *  injection. Returns the top-level `meta.rendered` value. */
function renderedNavigation(map: Record<string, string>): unknown {
    return metaMap([
        {
            key: 'navigation',
            value: metaMap(
                Object.entries(map).map(([k, v]) => ({
                    key: k,
                    value: ms(v),
                })),
            ),
        },
    ]);
}

describe('PreviewDocument chrome injection (Phase F.2)', () => {
    it('renders navbar HTML BEFORE quarto-content via dangerouslySetInnerHTML', () => {
        const navbarHtml =
            '<nav class="navbar navbar-expand-lg" data-test="nv">My Site</nav>';
        const { container } = mount({
            rendered: renderedNavigation({ navbar: navbarHtml }),
        });
        // The injected navbar is a sibling of #quarto-content, ordered before it.
        const quartoContent = container.querySelector('#quarto-content');
        expect(quartoContent).not.toBeNull();
        const nav = container.querySelector('nav.navbar[data-test="nv"]');
        expect(nav).not.toBeNull();
        // Order: nav comes before #quarto-content.
        expect(
            nav!.compareDocumentPosition(quartoContent!) &
                Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
    });

    it('renders sidebar HTML INSIDE quarto-content, before <main>', () => {
        const sidebarHtml =
            '<nav id="quarto-sidebar" class="sidebar" data-test="sb">Items</nav>';
        const { container } = mount({
            rendered: renderedNavigation({ sidebar: sidebarHtml }),
        });
        const quartoContent = container.querySelector('#quarto-content');
        expect(quartoContent).not.toBeNull();
        // Sidebar lives inside quarto-content.
        const sidebar = quartoContent!.querySelector('nav#quarto-sidebar');
        expect(sidebar).not.toBeNull();
        // Order: sidebar before <main>.
        const main = quartoContent!.querySelector('main');
        expect(
            sidebar!.compareDocumentPosition(main!) &
                Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
    });

    it('wraps TOC HTML in #quarto-margin-sidebar > nav#TOC > <h2>', () => {
        const tocInnerUl =
            '<ul><li data-test="toc-li"><a href="#sec">Section</a></li></ul>';
        const { container } = mount({
            navigation: metaMap([
                {
                    key: 'toc',
                    value: metaMap([{ key: 'title', value: ms('Contents') }]),
                },
            ]),
            rendered: renderedNavigation({ toc: tocInnerUl }),
        });
        // Wrapper structure mirrors template.rs:189-200.
        const margin = container.querySelector(
            'div#quarto-margin-sidebar.sidebar.margin-sidebar',
        );
        expect(margin).not.toBeNull();
        const tocNav = margin!.querySelector(
            'nav#TOC[role="doc-toc"].toc-active',
        );
        expect(tocNav).not.toBeNull();
        const h2 = tocNav!.querySelector('h2#toc-title');
        expect(h2).not.toBeNull();
        expect(h2!.textContent).toBe('Contents');
        // The injected <ul> is in the dangerouslySetInnerHTML wrapper div.
        expect(tocNav!.querySelector('li[data-test="toc-li"]')).not.toBeNull();
    });

    it('TOC: missing navigation.toc.title omits the <h2>', () => {
        const tocInnerUl = '<ul><li>Sec</li></ul>';
        const { container } = mount({
            // No `navigation.toc.title` → tocTitle is empty → no <h2>.
            rendered: renderedNavigation({ toc: tocInnerUl }),
        });
        const tocNav = container.querySelector('#quarto-margin-sidebar nav#TOC');
        expect(tocNav).not.toBeNull();
        expect(tocNav!.querySelector('h2#toc-title')).toBeNull();
    });

    it('renders page-navigation INSIDE main, after children', () => {
        const pageNavHtml =
            '<nav class="page-navigation" data-test="pn">prev | next</nav>';
        const { container } = mount(
            {
                rendered: renderedNavigation({ page_navigation: pageNavHtml }),
            },
            [PARA(STR('Body content here.'))],
        );
        const main = container.querySelector('main#quarto-document-content');
        expect(main).not.toBeNull();
        const pageNav = main!.querySelector('nav.page-navigation');
        expect(pageNav).not.toBeNull();
        // Order: paragraph before page-nav inside main.
        const para = main!.querySelector('p');
        expect(
            para!.compareDocumentPosition(pageNav!) &
                Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
    });

    it('renders footer AFTER quarto-content', () => {
        const footerHtml =
            '<footer class="footer" data-test="ft">my footer</footer>';
        const { container } = mount({
            rendered: renderedNavigation({ footer: footerHtml }),
        });
        const quartoContent = container.querySelector('#quarto-content');
        const footer = container.querySelector('footer.footer');
        expect(footer).not.toBeNull();
        expect(
            quartoContent!.compareDocumentPosition(footer!) &
                Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
    });

    // ──────────────────────────────────────────────────────────────
    // Chrome wrappers must not perturb the parent's CSS layout.
    //
    // Each slot uses `dangerouslySetInnerHTML`, which requires a host
    // element — a plain `<div>`. The native template (template.rs:185)
    // injects the chrome HTML with NO wrapper, so the chrome's root
    // element (`nav#quarto-sidebar`, etc.) is a direct child of
    // `#quarto-content`. The Quarto theme grid keys off that direct-
    // child relationship: `nav#quarto-sidebar` gets `grid-area:
    // content-top / page-start / page-bottom / body-start` only when
    // it's a grid item of `#quarto-content`. With an opaque `<div>`
    // wrapper, the wrapper becomes the grid item (with default
    // placement in the main content column), and the sidebar inside
    // it stacks below `<main>`. (bd-xdier)
    //
    // The slot wrapper must be `display: contents` so it's transparent
    // to the parent's layout. These tests guard that — either no
    // wrapper at all (the injected root IS the direct child) or a
    // `display: contents` wrapper.
    // ──────────────────────────────────────────────────────────────

    function wrapperIsLayoutTransparent(
        injected: Element,
        directParent: Element,
    ): boolean {
        // Two acceptable shapes: (a) injected element is itself the
        // direct child; (b) there's a wrapper, but it's display:contents
        // so it doesn't participate in the parent's grid/flex layout.
        if (injected.parentElement === directParent) return true;
        const wrapper = injected.parentElement;
        if (!wrapper) return false;
        if (wrapper.parentElement !== directParent) return false;
        return wrapper.style.display === 'contents';
    }

    it('sidebar slot wrapper is layout-transparent (no grid perturbation)', () => {
        const sidebarHtml =
            '<nav id="quarto-sidebar" class="sidebar" data-test="sb">Items</nav>';
        const { container } = mount({
            rendered: renderedNavigation({ sidebar: sidebarHtml }),
        });
        const quartoContent = container.querySelector('#quarto-content')!;
        const sidebar = container.querySelector('nav#quarto-sidebar')!;
        expect(wrapperIsLayoutTransparent(sidebar, quartoContent)).toBe(true);
    });

    it('navbar slot wrapper is layout-transparent', () => {
        const navbarHtml =
            '<nav class="navbar" data-test="nv">My Site</nav>';
        const { container } = mount({
            rendered: renderedNavigation({ navbar: navbarHtml }),
        });
        const nav = container.querySelector('nav.navbar[data-test="nv"]')!;
        // Navbar lives at the document-root level (sibling of #quarto-content).
        // The container's first-level child holds both the navbar's wrapper
        // and #quarto-content; whatever that is, the wrapper must be
        // layout-transparent within it.
        const navbarHost = nav.parentElement?.parentElement;
        expect(navbarHost).not.toBeNull();
        expect(wrapperIsLayoutTransparent(nav, navbarHost!)).toBe(true);
    });

    it('footer slot wrapper is layout-transparent', () => {
        const footerHtml =
            '<footer class="footer" data-test="ft">my footer</footer>';
        const { container } = mount({
            rendered: renderedNavigation({ footer: footerHtml }),
        });
        const footer = container.querySelector('footer.footer')!;
        const footerHost = footer.parentElement?.parentElement;
        expect(footerHost).not.toBeNull();
        expect(wrapperIsLayoutTransparent(footer, footerHost!)).toBe(true);
    });

    it('page-nav slot wrapper is layout-transparent inside <main>', () => {
        const pageNavHtml =
            '<nav class="page-navigation" data-test="pn">prev | next</nav>';
        const { container } = mount(
            {
                rendered: renderedNavigation({ page_navigation: pageNavHtml }),
            },
            [PARA(STR('Body content here.'))],
        );
        const main = container.querySelector('main#quarto-document-content')!;
        const pageNav = container.querySelector('nav.page-navigation')!;
        expect(wrapperIsLayoutTransparent(pageNav, main)).toBe(true);
    });

    it('absent meta.rendered.navigation.* keys → no chrome elements rendered', () => {
        const { container } = mount({}, [PARA(STR('plain doc'))]);
        // None of the chrome wrappers should exist.
        expect(container.querySelector('#quarto-margin-sidebar')).toBeNull();
        expect(container.querySelector('nav#quarto-sidebar')).toBeNull();
        expect(container.querySelector('nav.page-navigation')).toBeNull();
        expect(container.querySelector('footer.footer')).toBeNull();
        expect(container.querySelector('nav.navbar')).toBeNull();
    });

    it('sidebar-render body-classes hoists onto document.body', () => {
        // Phase F.2: when the user did NOT set top-level `body-classes`,
        // `meta.rendered.navigation.body-classes` (from sidebar-render)
        // is the source — same as Rust template.rs:419-428.
        // bd-elgxx (D6 react): color-mode class still appends after the
        // structural class (matches the Rust template helper).
        mount({
            rendered: metaMap([
                {
                    key: 'navigation',
                    value: metaMap([
                        { key: 'body-classes', value: ms('nav-sidebar floating') },
                    ]),
                },
            ]),
        });
        expect(document.body.className).toBe('nav-sidebar floating quarto-light');
    });

    it('user-set top-level body-classes still wins over sidebar-render', () => {
        // Mirror Rust template.rs:419 — user override always wins.
        mount({
            'body-classes': ms('user-bcs'),
            rendered: metaMap([
                {
                    key: 'navigation',
                    value: metaMap([
                        { key: 'body-classes', value: ms('nav-sidebar floating') },
                    ]),
                },
            ]),
        });
        // bd-elgxx (D6 react): color-mode class still appends after
        // the user's body-classes win.
        expect(document.body.className).toBe('user-bcs quarto-light');
    });

    it('header-includes (favicon link) lands in document.head with cleanup marker', () => {
        // The favicon transform appends a `<link rel="icon">` HTML
        // string to `meta.rendered.includes.header`. The
        // `HeaderIncludesEffect` hook parses it and inserts the
        // `<link>` into `document.head` with a `data-q2-header-include`
        // marker for the cleanup pass.
        const linkHtml =
            '<link rel="icon" href="/.quarto/project-artifacts/favicon.ico" type="image/x-icon" data-test="fv">';
        const { unmount } = mount({
            rendered: metaMap([
                {
                    key: 'includes',
                    value: metaMap([
                        {
                            key: 'header',
                            value: { t: 'MetaList', c: [ms(linkHtml)] },
                        },
                    ]),
                },
            ]),
        });

        const link = document.head.querySelector(
            'link[data-test="fv"][data-q2-header-include]',
        );
        expect(link).not.toBeNull();
        expect(link!.getAttribute('rel')).toBe('icon');

        // Unmount cleanup must remove the inserted node.
        unmount();
        expect(
            document.head.querySelector('link[data-test="fv"]'),
        ).toBeNull();
    });

    // bd-5oyk1xce: engine include-in-header can be executable (marimo's
    // islands module + its inline __MARIMO_EXPORT_CONTEXT__ marker). A
    // <script> parsed via innerHTML is inert; HeaderIncludesEffect must
    // re-materialize it so the browser runs it. jsdom (no runScripts) can't
    // observe execution — that is proven by the real-browser e2e
    // (engine-capture-splice-marimo). These bind the re-materialization
    // logic + the head delivery/cleanup wiring.
    it('rematerializeScript returns a fresh executable <script> copying attrs + inline body', () => {
        // Build an INERT script the way innerHTML does (the exact node
        // HeaderIncludesEffect starts from).
        const wrapper = document.createElement('div');
        wrapper.innerHTML =
            '<script type="module" src="https://cdn.jsdelivr.net/npm/@marimo-team/islands/main.js" data-x="1"></script>';
        const inert = wrapper.firstElementChild as HTMLScriptElement;

        const fresh = rematerializeScript(inert);

        // A DISTINCT node (the whole point — the inert one won't execute).
        expect(fresh).not.toBe(inert);
        expect(fresh.tagName).toBe('SCRIPT');
        expect(fresh.getAttribute('type')).toBe('module');
        expect(fresh.getAttribute('src')).toBe(
            'https://cdn.jsdelivr.net/npm/@marimo-team/islands/main.js',
        );
        expect(fresh.getAttribute('data-x')).toBe('1');

        // Inline body is copied verbatim (e.g. __MARIMO_EXPORT_CONTEXT__).
        const inlineWrapper = document.createElement('div');
        inlineWrapper.innerHTML =
            '<script>window.__MARIMO_EXPORT_CONTEXT__ = { session: "s" };</script>';
        const inlineFresh = rematerializeScript(
            inlineWrapper.firstElementChild as HTMLScriptElement,
        );
        expect(inlineFresh.textContent).toBe(
            'window.__MARIMO_EXPORT_CONTEXT__ = { session: "s" };',
        );
    });

    it('header-includes <script> lands in document.head (re-materialized) with cleanup', () => {
        const scriptHtml =
            '<script type="module" src="https://cdn.jsdelivr.net/npm/@marimo-team/islands@0.23.13/dist/main.js" data-test="islands"></script>';
        const { unmount } = mount({
            rendered: metaMap([
                {
                    key: 'includes',
                    value: metaMap([
                        {
                            key: 'header',
                            value: { t: 'MetaList', c: [ms(scriptHtml)] },
                        },
                    ]),
                },
            ]),
        });

        const script = document.head.querySelector(
            'script[data-test="islands"][data-q2-header-include]',
        ) as HTMLScriptElement | null;
        expect(script).not.toBeNull();
        expect(script!.getAttribute('type')).toBe('module');
        expect(script!.getAttribute('src')).toBe(
            'https://cdn.jsdelivr.net/npm/@marimo-team/islands@0.23.13/dist/main.js',
        );

        unmount();
        expect(
            document.head.querySelector('script[data-test="islands"]'),
        ).toBeNull();
    });

    it('minimal mode skips chrome injection (matches Rust minimal template)', () => {
        // Minimal mode is the rust template that has no chrome
        // substitutions. PreviewDocument's minimal branch returns a
        // <Fragment> with just title + children.
        const { container } = mount({
            minimal: mb(true),
            rendered: renderedNavigation({
                navbar: '<nav class="navbar" data-test="nv">x</nav>',
                footer: '<footer class="footer" data-test="ft">y</footer>',
            }),
        });
        // No chrome elements injected in minimal mode.
        expect(container.querySelector('nav.navbar')).toBeNull();
        expect(container.querySelector('footer.footer')).toBeNull();
    });
});

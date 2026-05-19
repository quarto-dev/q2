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

    it('body-classes: custom-cls → document.body.className === "custom-cls" (no fullcontent)', () => {
        mount({ 'body-classes': ms('custom-cls') });
        expect(document.body.className).toBe('custom-cls');
    });

    it('body-classes: "" (empty string) opts out — body has no classes', () => {
        // Pandoc-falsy parity: $body-classes$ template substitution
        // emits the empty string verbatim, so empty string opts out
        // of the literal `fullcontent` default. Only undefined
        // (missing key) triggers the fallback.
        mount({ 'body-classes': ms('') });
        expect(document.body.className).toBe('');
    });

    it('default → document.body.className === "fullcontent"', () => {
        mount({});
        expect(document.body.className).toBe('fullcontent');
    });

    it('cleanup: unmount restores the pre-mount body.className', () => {
        document.body.className = 'pre-existing';
        const { unmount } = mount({ 'body-classes': ms('mid') });
        expect(document.body.className).toBe('mid');
        unmount();
        expect(document.body.className).toBe('pre-existing');
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
        expect(document.body.className).toBe('nav-sidebar floating');
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
        expect(document.body.className).toBe('user-bcs');
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

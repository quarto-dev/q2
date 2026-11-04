/**
 * The dashboard preview (plan 2026-11-03-dashboard-format-port.md, Phase
 * 8): the view nodes keep the render's attributes and add the behavior
 * bslib / Bootstrap JS gives the render, and the shell builds the navbar
 * from `rendered.dashboard.*`.
 */
import { describe, expect, it } from 'vitest';
import { fireEvent, render } from '@testing-library/react';
import { Ast } from '../../../framework';
import type { FormatRegistry, PandocAST } from '../../../framework';
import { previewRegistry } from '../../registry';
import { DashboardDocument } from './DashboardDocument';

const STR = (c: string) => ({ t: 'Str', c });
const PLAIN = (...inlines: unknown[]) => ({ t: 'Plain', c: inlines });
const DIV = (classes: string[], blocks: unknown[], id = '') => ({
    t: 'Div',
    c: [[id, classes, []], blocks],
});
const CUSTOM = (
    type_name: string,
    attr: [string, string[], [string, string][]],
    plain_data: Record<string, unknown>,
    slots: Record<string, unknown[]>,
) => ({
    t: 'CustomBlock',
    type_name,
    attr,
    plain_data,
    slots: Object.fromEntries(
        Object.entries(slots).map(([k, v]) => [k, { kind: 'blocks', value: v }]),
    ),
});

const MS = (s: string) => ({ t: 'MetaString', c: s });
const MAP = (o: Record<string, unknown>) => ({ t: 'MetaMap', c: o });
const LIST = (xs: unknown[]) => ({ t: 'MetaList', c: xs });

function mount(blocks: unknown[], meta: Record<string, unknown> = {}, dashboard = false) {
    const ast: PandocAST = {
        'pandoc-api-version': [1, 23, 0],
        meta: meta as PandocAST['meta'],
        blocks: blocks as PandocAST['blocks'],
    };
    const registry = (dashboard
        ? { ...previewRegistry, Ast: DashboardDocument }
        : previewRegistry) as FormatRegistry;
    return render(
        <Ast
            astJson={JSON.stringify(ast)}
            currentFilePath="/project/test.qmd"
            onNavigateToDocument={() => {}}
            setAst={() => {}}
            registry={registry}
        />,
    );
}

const card = (expandable: boolean, content: unknown[]) =>
    CUSTOM(
        'DashboardCardView',
        ['', ['card', 'bslib-card', 'html-fill-container'], expandable ? [['data-full-screen', 'false']] : []],
        { expandable },
        { content },
    );

describe('dashboard view nodes', () => {
    it('a card keeps its attributes and toggles full screen', () => {
        const { container } = mount([card(true, [DIV(['card-body'], [PLAIN(STR('Body'))])])]);
        const el = container.querySelector('div.bslib-card')!;
        expect(el.getAttribute('data-full-screen')).toBe('false');
        expect(el.querySelector('.card-body')?.textContent).toBe('Body');
        fireEvent.click(el.querySelector('.bslib-full-screen-enter')!);
        expect(el.getAttribute('data-full-screen')).toBe('true');
        expect(document.body.classList.contains('bslib-has-full-screen')).toBe(true);
        fireEvent.click(container.querySelector('#bslib-full-screen-overlay .bslib-full-screen-exit')!);
        expect(el.getAttribute('data-full-screen')).toBe('false');
        expect(document.body.classList.contains('bslib-has-full-screen')).toBe(false);
    });

    it('a card without expand has no expand control', () => {
        const { container } = mount([card(false, [])]);
        expect(container.querySelector('.bslib-full-screen-enter')).toBeNull();
    });

    it('the sidebar layout collapses', () => {
        const { container } = mount([
            CUSTOM(
                'DashboardSidebarLayout',
                ['', ['bslib-sidebar-layout', 'html-fill-item'], [['data-bslib-sidebar-open', 'desktop']]],
                { sidebar_id: 'bslib-sidebar-1' },
                {
                    main: [DIV(['main'], [PLAIN(STR('Main'))])],
                    sidebar: [DIV([], [PLAIN(STR('Side'))])],
                },
            ),
        ]);
        const layout = container.querySelector('.bslib-sidebar-layout')!;
        const aside = layout.querySelector('aside#bslib-sidebar-1')!;
        expect(aside.textContent).toBe('Side');
        expect(layout.querySelector('.main')?.textContent).toBe('Main');
        const toggle = layout.querySelector('button.collapse-toggle')!;
        expect(toggle.getAttribute('aria-controls')).toBe('bslib-sidebar-1');
        fireEvent.click(toggle);
        expect(layout.classList.contains('sidebar-collapsed')).toBe(true);
        expect(aside.hasAttribute('hidden')).toBe(true);
        expect(toggle.getAttribute('aria-expanded')).toBe('false');
    });

    it('a tabset card switches tabs', () => {
        const nav = CUSTOM('DashboardTabNav', ['', [], []], {
            tabset_id: 'card-tabset-1',
            tabs: [
                { id: 'card-tabset-1-1', title: 'One' },
                { id: 'card-tabset-1-2', title: 'Two' },
            ],
        }, {});
        const pane = (n: number) =>
            CUSTOM(
                'DashboardTabPane',
                [`card-tabset-1-${n}`, n === 1 ? ['tab-pane', 'active', 'show'] : ['tab-pane'], [['role', 'tabpanel']]],
                { id: `card-tabset-1-${n}` },
                { content: [PLAIN(STR(`P${n}`))] },
            );
        const { container } = mount([
            card(false, [DIV(['card-header'], [nav]), DIV(['tab-content'], [pane(1), pane(2)])]),
        ]);
        const p1 = container.querySelector('#card-tabset-1-1')!;
        const p2 = container.querySelector('#card-tabset-1-2')!;
        expect(p1.classList.contains('active')).toBe(true);
        expect(p2.classList.contains('active')).toBe(false);
        const links = container.querySelectorAll('ul.nav-tabs a.nav-link');
        expect(links).toHaveLength(2);
        fireEvent.click(links[1]);
        expect(p1.classList.contains('active')).toBe(false);
        expect(p2.classList.contains('active') && p2.classList.contains('show')).toBe(true);
        expect(links[1].getAttribute('aria-selected')).toBe('true');
    });
});

describe('dashboard shell', () => {
    const page = (id: string, first: boolean) =>
        CUSTOM(
            'DashboardPageView',
            [id, first ? ['dashboard-page', 'tab-pane', 'show', 'active'] : ['dashboard-page', 'tab-pane'], []],
            { id },
            { content: [PLAIN(STR(id.toUpperCase()))] },
        );
    const meta = {
        title: MS('Dash'),
        rendered: MAP({
            dashboard: MAP({
                pages: LIST([
                    MAP({ id: MS('one'), text: MS('One'), active: { t: 'MetaBool', c: true }, scrolling: MS('false') }),
                    MAP({ id: MS('two'), text: MS('Two'), active: { t: 'MetaBool', c: false }, scrolling: MS('false') }),
                ]),
                'nav-buttons': LIST([MAP({ href: MS('https://x.test/?u=|url|'), icon: MS('twitter'), title: MS('Twitter') })]),
                'body-classes': MS('dashboard-sidebar'),
                'content-classes': MS('dashboard-sidebar-container'),
            }),
        }),
    };

    it('builds the navbar and content area and switches pages', () => {
        const { container } = mount([DIV(['bslib-grid', 'tab-content'], [page('one', true), page('two', false)])], meta, true);
        expect(container.querySelector('.navbar-title-text')?.textContent).toBe('Dash');
        const content = container.querySelector('#quarto-document-content')!;
        expect(content.className).toContain('quarto-dashboard-pages');
        expect(content.className).toContain('dashboard-sidebar-container');
        expect(document.body.classList.contains('quarto-dashboard')).toBe(true);
        expect(document.body.classList.contains('dashboard-sidebar')).toBe(true);

        const link = container.querySelector('#dashboard-collapse .quarto-dashboard-links a')!;
        expect(link.getAttribute('href')).not.toContain('|url|');

        const one = container.querySelector('#one')!;
        const two = container.querySelector('#two')!;
        expect(one.classList.contains('active')).toBe(true);
        fireEvent.click(container.querySelector('#tab-two')!);
        expect(two.classList.contains('active')).toBe(true);
        expect(one.classList.contains('active')).toBe(false);
        expect(window.location.hash).toBe('#two');
    });
});

describe('dashboard shell in a website', () => {
    it('wraps the dashboard header in the site header and adds the footer', () => {
        const meta = {
            title: MS('Dash'),
            rendered: MAP({
                navigation: MAP({
                    navbar: MS('<nav class="navbar" id="site-nav"><a class="nav-link" href="index.html">Home</a></nav>'),
                    footer: MS('<footer class="footer"><div class="nav-footer">FOOT</div></footer>'),
                }),
            }),
        };
        const { container } = mount([DIV(['bslib-grid'], [PLAIN(STR('Body'))])], meta, true);
        const site = container.querySelector('header#quarto-header.fixed-top')!;
        expect(site.querySelector('#site-nav')).not.toBeNull();
        expect(site.querySelector('header#quarto-dashboard-header')).not.toBeNull();
        const content = container.querySelector('div#quarto-content')!;
        expect(content.className).toContain('quarto-container');
        expect(content.className).toContain('page-navbar');
        expect(container.querySelector('#quarto-document-content')).toBeNull();
        expect(container.querySelector('#quarto-skip-link')?.getAttribute('href')).toBe('#quarto-content');
        expect(container.querySelector('footer.footer')?.textContent).toBe('FOOT');
        expect(document.body.classList.contains('nav-fixed')).toBe(true);
    });
});

/**
 * Vitest tests for the fixed scroll-away header in q2-preview
 * (bd-ersobfbt): the React-owned `<header id="quarto-header">` wrapper,
 * its `headroom fixed-top` classes, the `nav-fixed` body class, and the
 * wrapper's DOM stability across AST re-posts (the property that keeps
 * a live Headroom instance bound to a valid element).
 *
 * Mounts via `<Ast>` with `previewRegistry`, mirroring
 * `PreviewDocument.integration.test.tsx`. The expected structure is the
 * Rust template's: `template.rs::QUARTO_HEADER_PARTIAL` + the
 * body-class compose in `render_with_compiled_template` — preview and
 * render must agree byte-for-byte on classes.
 */

import { describe, expect, it, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/react';
import { Ast } from '../framework';
import type { PandocAST } from '../framework';
import { previewRegistry } from './registry';

function astJson(meta: Record<string, unknown>, blocks: unknown[] = []): string {
    const ast: PandocAST = {
        'pandoc-api-version': [1, 23, 0],
        meta,
        blocks: blocks as never[],
    };
    return JSON.stringify(ast);
}

function mount(meta: Record<string, unknown>, blocks: unknown[] = []) {
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
const PARA = (...inlines: unknown[]) => ({ t: 'Para', c: inlines });
const ms = (c: string) => ({ t: 'MetaString', c });
const mb = (c: boolean) => ({ t: 'MetaBool', c });
const mm = (entries: Record<string, unknown>) => ({
    t: 'MetaMap',
    c: Object.entries(entries).map(([key, value]) => ({ key, value })),
});

const NAVBAR_HTML = '<nav class="navbar navbar-expand-lg"><div>NAVBAR_BODY</div></nav>';
const SECONDARY_HTML = '<nav class="quarto-secondary-nav"><div>SECONDARY_BODY</div></nav>';

const navMeta = (nav: Record<string, unknown>, extra: Record<string, unknown> = {}) => ({
    rendered: mm({ navigation: mm(nav), ...extra }),
});

let priorBodyClass: string;
beforeEach(() => {
    priorBodyClass = document.body.className;
    document.body.className = '';
});
afterEach(() => {
    document.body.className = priorBodyClass;
});

describe('q2-preview #quarto-header wrapper (bd-ersobfbt)', () => {
    it('navbar → React-owned <header id="quarto-header" class="headroom fixed-top"> wraps the navbar, before #quarto-content', () => {
        const { container } = mount(navMeta({ navbar: ms(NAVBAR_HTML) }), [
            PARA(STR('hello')),
        ]);

        const header = container.querySelector('header#quarto-header');
        expect(header).not.toBeNull();
        expect(header!.className).toBe('headroom fixed-top');
        expect(header!.querySelector('nav.navbar')).not.toBeNull();

        // Header precedes #quarto-content (template.rs order).
        const content = container.querySelector('#quarto-content');
        expect(content).not.toBeNull();
        expect(
            header!.compareDocumentPosition(content!) &
                Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
    });

    it('secondary nav only → header exists and wraps it', () => {
        const { container } = mount(
            navMeta({ 'secondary-nav': ms(SECONDARY_HTML) }),
        );
        const header = container.querySelector('header#quarto-header');
        expect(header).not.toBeNull();
        expect(header!.querySelector('nav.quarto-secondary-nav')).not.toBeNull();
    });

    it('navbar + secondary nav → both inside the header, navbar first', () => {
        const { container } = mount(
            navMeta({
                navbar: ms(NAVBAR_HTML),
                'secondary-nav': ms(SECONDARY_HTML),
            }),
        );
        const header = container.querySelector('header#quarto-header')!;
        const navbar = header.querySelector('nav.navbar')!;
        const secondary = header.querySelector('nav.quarto-secondary-nav')!;
        expect(
            navbar.compareDocumentPosition(secondary) &
                Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
    });

    it('no navbar and no secondary nav → no header wrapper', () => {
        const { container } = mount({}, [PARA(STR('hello'))]);
        expect(container.querySelector('#quarto-header')).toBeNull();
    });

    it('pinned navbar → header carries data-headroom-pinned (quarto-nav.js skips Headroom)', () => {
        // Preview analogue of the native `pinned:` opt-out: the native
        // build simply doesn't ship headroom.min.js; the preview bundle
        // always contains it, so the header is tagged and quarto-nav.js
        // declines to bind Headroom to a tagged header.
        const { container } = mount({
            navigation: mm({ navbar: mm({ pinned: mb(true) }) }),
            ...navMeta({ navbar: ms(NAVBAR_HTML) }),
        });
        const header = container.querySelector('header#quarto-header');
        expect(header!.getAttribute('data-headroom-pinned')).toBe('true');
    });

    it('pinned sidebar → header carries data-headroom-pinned', () => {
        const { container } = mount({
            navigation: mm({ sidebar: mm({ pinned: mb(true) }) }),
            ...navMeta({ navbar: ms(NAVBAR_HTML) }),
        });
        const header = container.querySelector('header#quarto-header');
        expect(header!.getAttribute('data-headroom-pinned')).toBe('true');
    });

    it('unpinned → no data-headroom-pinned attribute', () => {
        const { container } = mount(navMeta({ navbar: ms(NAVBAR_HTML) }));
        const header = container.querySelector('header#quarto-header');
        expect(header!.hasAttribute('data-headroom-pinned')).toBe(false);
    });

    it('banner mode → quarto-banner appended after headroom fixed-top', () => {
        const { container } = mount(
            navMeta({ navbar: ms(NAVBAR_HTML) }, { 'title-block-banner': mb(true) }),
            [PARA(STR('hello'))],
        );
        const header = container.querySelector('header#quarto-header');
        expect(header!.className).toBe('headroom fixed-top quarto-banner');
    });
});

describe('q2-preview nav-fixed body class (bd-ersobfbt)', () => {
    it('navbar rendered → nav-fixed appended after the structural class', () => {
        // Mirrors Rust: no TOC → structural fullcontent, then nav-fixed,
        // then the color-mode class.
        mount(navMeta({ navbar: ms(NAVBAR_HTML) }), [PARA(STR('hello'))]);
        expect(document.body.className).toBe('fullcontent nav-fixed quarto-light');
    });

    it('navbar + sidebar body-classes → accumulates, not replaces', () => {
        mount(
            navMeta({
                navbar: ms(NAVBAR_HTML),
                'body-classes': ms('nav-sidebar floating'),
            }),
        );
        expect(document.body.className).toBe(
            'nav-sidebar floating nav-fixed quarto-light',
        );
    });

    it('secondary nav only (no navbar) → NO nav-fixed (Q1 requires nav.navbar)', () => {
        mount(navMeta({ 'secondary-nav': ms(SECONDARY_HTML) }));
        expect(document.body.className).not.toContain('nav-fixed');
    });

    it('user top-level body-classes wins wholesale — no nav-fixed append', () => {
        mount({
            'body-classes': ms('custom-layout'),
            ...navMeta({ navbar: ms(NAVBAR_HTML) }),
        });
        expect(document.body.className).not.toContain('nav-fixed');
        expect(document.body.className).toContain('custom-layout');
    });
});

describe('q2-preview header DOM stability (bd-ersobfbt)', () => {
    it('header element identity survives an AST re-post with changed chrome HTML', () => {
        // The wrapper must be React-owned (outside dangerouslySetInnerHTML)
        // so a live Headroom instance stays bound to a valid element when
        // the chrome HTML inside it is replaced (chromeSlots.tsx contract).
        const { container, rerender } = render(
            <Ast
                astJson={astJson(navMeta({ navbar: ms(NAVBAR_HTML) }), [
                    PARA(STR('one')),
                ])}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={() => {}}
                setAst={() => {}}
                registry={previewRegistry}
            />,
        );
        const before = container.querySelector('header#quarto-header');
        expect(before).not.toBeNull();

        rerender(
            <Ast
                astJson={astJson(
                    navMeta({
                        navbar: ms(
                            '<nav class="navbar navbar-expand-lg"><div>CHANGED</div></nav>',
                        ),
                    }),
                    [PARA(STR('two'))],
                )}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={() => {}}
                setAst={() => {}}
                registry={previewRegistry}
            />,
        );
        const after = container.querySelector('header#quarto-header');
        expect(after).not.toBeNull();
        expect(after).toBe(before);
        expect(after!.textContent).toContain('CHANGED');
    });
});

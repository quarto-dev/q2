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

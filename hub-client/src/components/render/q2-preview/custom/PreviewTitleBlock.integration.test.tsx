/**
 * Vitest tests for `PreviewTitleBlock` (Plan 2D Phase 7.4).
 *
 * Mounts via `<Ast>` with `previewRegistry` so the title block is
 * resolved through the registry's `__title_block__` synthetic key
 * the same way it would be in production.
 *
 * Asserts the rendered DOM matches the Rust HTML template's title
 * block at `crates/quarto-core/src/template.rs:211-240` byte-for-byte,
 * including the deliberately-replicated quirks (date suppressed
 * without author; multi-author empty-string-join).
 */

import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest';
import { render } from '@testing-library/react';
import { Ast } from '@quarto/preview-renderer/framework';
import type { FormatRegistry, PandocAST } from '@quarto/preview-renderer/framework';
import { previewRegistry } from '../registry';
import { PreviewTitleBlock } from './PreviewTitleBlock';

function astJson(meta: Record<string, unknown>, blocks: any[] = []): string {
    const ast: PandocAST = {
        'pandoc-api-version': [1, 23, 0],
        meta,
        blocks: blocks as any,
    };
    return JSON.stringify(ast);
}

function mount(
    meta: Record<string, unknown>,
    blocks: any[] = [],
    registry: FormatRegistry = previewRegistry,
) {
    return render(
        <Ast
            astJson={astJson(meta, blocks)}
            currentFilePath="/project/test.qmd"
            onNavigateToDocument={() => {}}
            setAst={() => {}}
            registry={registry}
        />,
    );
}

const STR = (c: string) => ({ t: 'Str', c });
const PARA = (...inlines: any[]) => ({ t: 'Para', c: inlines });
const ms = (c: string) => ({ t: 'MetaString', c });
const ml = (...items: any[]) => ({ t: 'MetaList', c: items });
const mi = (...inlines: any[]) => ({ t: 'MetaInlines', c: inlines });

let priorBodyClass: string;
let priorTitle: string;
beforeEach(() => {
    priorBodyClass = document.body.className;
    priorTitle = document.title;
});
afterEach(() => {
    document.body.className = priorBodyClass;
    document.title = priorTitle;
});

describe('PreviewTitleBlock — required elements', () => {
    it('no title → renders null (no <header>)', () => {
        const { container } = mount({}, [PARA(STR('body'))]);
        expect(
            container.querySelector('header#title-block-header'),
        ).toBeNull();
    });

    it('title only → <header> + <h1 class="title">, nothing optional', () => {
        const { container } = mount({ title: ms('Doc') });
        const header = container.querySelector('header#title-block-header');
        expect(header).not.toBeNull();
        expect(header!.className).toBe('quarto-title-block default');
        const h1 = header!.querySelector('div.quarto-title > h1.title');
        expect(h1).not.toBeNull();
        expect(h1!.textContent).toBe('Doc');
        expect(header!.querySelector('p.subtitle')).toBeNull();
        expect(header!.querySelector('div.quarto-title-meta')).toBeNull();
        expect(header!.querySelector('div.abstract')).toBeNull();
    });

    it('title + subtitle → adds <p class="subtitle">', () => {
        const { container } = mount({
            title: ms('Doc'),
            subtitle: ms('Sub'),
        });
        const subtitle = container.querySelector(
            'header#title-block-header div.quarto-title > p.subtitle',
        );
        expect(subtitle).not.toBeNull();
        expect(subtitle!.textContent).toBe('Sub');
    });

    it('title + author (string) → exactly one .quarto-title-meta-author block', () => {
        const { container } = mount({
            title: ms('Doc'),
            author: ms('Jane Doe'),
        });
        const meta = container.querySelector('div.quarto-title-meta');
        expect(meta).not.toBeNull();
        const authors = container.querySelectorAll(
            'div.quarto-title-meta-author',
        );
        expect(authors.length).toBe(1);
        expect(
            authors[0].querySelector('.quarto-title-meta-heading')!.textContent,
        ).toBe('Author');
        expect(
            authors[0].querySelector('.quarto-title-meta-contents')!.textContent,
        ).toBe('Jane Doe');
    });

    it('title + author (MetaList of two) → ONE author block with empty-join content', () => {
        const { container } = mount({
            title: ms('Doc'),
            author: ml(ms('Alice'), ms('Bob')),
        });
        const authors = container.querySelectorAll(
            'div.quarto-title-meta-author',
        );
        expect(authors.length).toBe(1);
        expect(
            authors[0].querySelector('.quarto-title-meta-contents')!.textContent,
        ).toBe('AliceBob');
    });

    it('title + author + date → date sibling of author inside .quarto-title-meta', () => {
        const { container } = mount({
            title: ms('Doc'),
            author: ms('Jane'),
            date: ms('2026-05-10'),
        });
        const meta = container.querySelector('div.quarto-title-meta');
        expect(meta).not.toBeNull();
        const author = meta!.querySelector(
            ':scope > div.quarto-title-meta-author',
        );
        const date = meta!.querySelector(
            ':scope > div.quarto-title-meta-date',
        );
        expect(author).not.toBeNull();
        expect(date).not.toBeNull();
        expect(
            date!.querySelector('.quarto-title-meta-heading')!.textContent,
        ).toBe('Published');
        expect(
            date!.querySelector('.quarto-title-meta-contents')!.textContent,
        ).toBe('2026-05-10');
    });

    it('title + date but NO author → date does NOT render (Rust quirk locked)', () => {
        const { container } = mount({
            title: ms('Doc'),
            date: ms('2026-05-10'),
        });
        expect(container.querySelector('div.quarto-title-meta')).toBeNull();
        expect(
            container.querySelector('div.quarto-title-meta-date'),
        ).toBeNull();
    });

    it('title + abstract → adds <div class="abstract"> with abstract-title heading', () => {
        const { container } = mount({
            title: ms('Doc'),
            abstract: ms('A short summary.'),
        });
        const abstract = container.querySelector('div.abstract');
        expect(abstract).not.toBeNull();
        expect(
            abstract!.querySelector('.abstract-title')!.textContent,
        ).toBe('Abstract');
        expect(abstract!.textContent).toContain('A short summary.');
    });

    it('title with inline emphasis → renders as plain text (matches Rust)', () => {
        const { container } = mount({
            title: mi(STR('Hello'), { t: 'Space' }, {
                t: 'Emph',
                c: [STR('World')],
            }),
        });
        const h1 = container.querySelector('h1.title');
        expect(h1).not.toBeNull();
        expect(h1!.textContent).toBe('Hello World');
        // No <em> inside the h1 — emphasis is stripped to match Rust.
        expect(h1!.querySelector('em')).toBeNull();
    });
});

describe('PreviewTitleBlock — Pandoc-falsy semantics', () => {
    it('empty-string title → renders null (no <header>)', () => {
        const { container } = mount({ title: ms('') });
        expect(
            container.querySelector('header#title-block-header'),
        ).toBeNull();
    });

    it('title set + empty-string subtitle → <p class="subtitle"> is NOT rendered', () => {
        const { container } = mount({ title: ms('Doc'), subtitle: ms('') });
        expect(container.querySelector('p.subtitle')).toBeNull();
    });

    it('title set + empty-string author → .quarto-title-meta is NOT rendered', () => {
        const { container } = mount({ title: ms('Doc'), author: ms('') });
        expect(container.querySelector('div.quarto-title-meta')).toBeNull();
    });

    it('title + author + empty-string date → date sub-block is NOT rendered', () => {
        const { container } = mount({
            title: ms('Doc'),
            author: ms('Jane'),
            date: ms(''),
        });
        expect(container.querySelector('div.quarto-title-meta')).not.toBeNull();
        expect(
            container.querySelector('div.quarto-title-meta-date'),
        ).toBeNull();
    });

    it('title set + empty-string abstract → .abstract is NOT rendered', () => {
        const { container } = mount({ title: ms('Doc'), abstract: ms('') });
        expect(container.querySelector('div.abstract')).toBeNull();
    });

    it('title + author = MetaList ["Alice", ""] → one block with content "Alice"', () => {
        const { container } = mount({
            title: ms('Doc'),
            author: ml(ms('Alice'), ms('')),
        });
        const authors = container.querySelectorAll(
            'div.quarto-title-meta-author',
        );
        expect(authors.length).toBe(1);
        expect(
            authors[0].querySelector('.quarto-title-meta-contents')!.textContent,
        ).toBe('Alice');
    });
});

describe('PreviewTitleBlock — user override via registry', () => {
    it('full replacement → stub renders, built-in <header> is absent', () => {
        const StubTitleBlock = vi.fn(() => (
            <div data-testid="custom-title">x</div>
        ));
        const overrideRegistry: FormatRegistry = {
            ...previewRegistry,
            __title_block__: StubTitleBlock as any,
        };
        const { container } = mount(
            { title: ms('Doc') },
            [PARA(STR('body'))],
            overrideRegistry,
        );
        expect(
            container.querySelector('[data-testid="custom-title"]'),
        ).not.toBeNull();
        expect(
            container.querySelector('header#title-block-header'),
        ).toBeNull();
        expect(StubTitleBlock).toHaveBeenCalled();
    });

    it('composing the default → built-in <header> AND user extra both render', () => {
        // Mirrors the §8 composition idiom: the user override calls
        // the exposed `PreviewTitleBlock` (via the global in prod,
        // direct import here) and emits a sibling element. Locks
        // that the built-in is composable and remains so.
        const ComposedTitleBlock = (props: any) => (
            <>
                <PreviewTitleBlock {...props} />
                <div data-testid="extra">e</div>
            </>
        );
        const overrideRegistry: FormatRegistry = {
            ...previewRegistry,
            __title_block__: ComposedTitleBlock,
        };
        const { container } = mount(
            { title: ms('Doc') },
            [],
            overrideRegistry,
        );
        expect(
            container.querySelector('header#title-block-header'),
        ).not.toBeNull();
        expect(
            container.querySelector('[data-testid="extra"]'),
        ).not.toBeNull();
    });
});

/**
 * Vitest tests for `PreviewTitleBlock` (Plan 2D Phase 7.4; markup
 * updated by the title-block parity epic bd-gx9cic8z P1).
 *
 * Mounts via `<Ast>` with `previewRegistry` so the title block is
 * resolved through the registry's `__title_block__` synthetic key
 * the same way it would be in production.
 *
 * Asserts the rendered DOM matches the Rust built-in `title-block` /
 * `title-metadata` template partials (`TITLE_BLOCK_PARTIAL` /
 * `TITLE_METADATA_PARTIAL` in `crates/quarto-core/src/template.rs`).
 *
 * The component consumes the metadata the pipeline's
 * `AuthorsNormalizeTransform` derives (`rendered.has-title-block`,
 * `by-author`, `labels`); the `derived(…)` helper below builds meta
 * the way the pipeline delivers it.
 */

import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest';
import { render } from '@testing-library/react';
import { Ast } from '../../framework';
import type { FormatRegistry, PandocAST } from '../../framework';
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
const mb = (c: boolean) => ({ t: 'MetaBool', c });
const mm = (entries: Record<string, unknown>) => ({
    t: 'MetaMap',
    c: Object.entries(entries).map(([key, value]) => ({ key, value })),
});

/** Normalized `by-author` entry list, as `AuthorsNormalizeTransform` writes it. */
const byAuthor = (...names: string[]) =>
    ml(...names.map((n) => mm({ name: mm({ literal: ms(n) }) })));

/**
 * Wrap raw meta with the derived keys the pipeline's
 * `AuthorsNormalizeTransform` writes before the component runs:
 * `rendered.has-title-block` and (when authors given) `by-author`.
 */
function derived(
    meta: Record<string, unknown>,
    authors: string[] = [],
): Record<string, unknown> {
    return {
        ...meta,
        ...(authors.length > 0 ? { 'by-author': byAuthor(...authors) } : {}),
        rendered: mm({ 'has-title-block': mb(true) }),
    };
}

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
    it('no derived has-title-block flag → renders null (no <header>)', () => {
        const { container } = mount({}, [PARA(STR('body'))]);
        expect(
            container.querySelector('header#title-block-header'),
        ).toBeNull();
    });

    it('title only → <header> + <h1 class="title"> + empty meta grid', () => {
        const { container } = mount(derived({ title: ms('Doc') }));
        const header = container.querySelector('header#title-block-header');
        expect(header).not.toBeNull();
        expect(header!.className).toBe('quarto-title-block default');
        const h1 = header!.querySelector('div.quarto-title > h1.title');
        expect(h1).not.toBeNull();
        expect(h1!.textContent).toBe('Doc');
        expect(header!.querySelector('p.subtitle')).toBeNull();
        // Q1 parity: the quarto-title-meta grid div is always emitted
        // (empty when there are no cells).
        const grid = header!.querySelector('div.quarto-title-meta');
        expect(grid).not.toBeNull();
        expect(grid!.children.length).toBe(0);
        expect(header!.querySelector('div.abstract')).toBeNull();
    });

    it('title + subtitle → adds <p class="subtitle lead">', () => {
        const { container } = mount(
            derived({ title: ms('Doc'), subtitle: ms('Sub') }),
        );
        const subtitle = container.querySelector(
            'header#title-block-header div.quarto-title > p.subtitle.lead',
        );
        expect(subtitle).not.toBeNull();
        expect(subtitle!.textContent).toBe('Sub');
    });

    it('one author → bare-div cell, "Author" heading, <p>-wrapped name', () => {
        const { container } = mount(
            derived({ title: ms('Doc') }, ['Jane Doe']),
        );
        const grid = container.querySelector('div.quarto-title-meta');
        expect(grid).not.toBeNull();
        // Q1 parity: grid children are bare divs — the legacy
        // quarto-title-meta-author/-date classes are gone (the former
        // is reserved for the P2 affiliations grid).
        expect(
            container.querySelector('div.quarto-title-meta-author'),
        ).toBeNull();
        const cells = grid!.querySelectorAll(':scope > div');
        expect(cells.length).toBe(1);
        expect(
            cells[0].querySelector('.quarto-title-meta-heading')!.textContent,
        ).toBe('Author');
        const names = cells[0].querySelectorAll(
            '.quarto-title-meta-contents > p',
        );
        expect(names.length).toBe(1);
        expect(names[0].textContent).toBe('Jane Doe');
    });

    it('two authors → "Authors" heading and one <p> per author', () => {
        const { container } = mount(
            derived({ title: ms('Doc') }, ['Alice', 'Bob']),
        );
        const grid = container.querySelector('div.quarto-title-meta');
        const cells = grid!.querySelectorAll(':scope > div');
        expect(cells.length).toBe(1);
        expect(
            cells[0].querySelector('.quarto-title-meta-heading')!.textContent,
        ).toBe('Authors');
        const names = cells[0].querySelectorAll(
            '.quarto-title-meta-contents > p',
        );
        expect(names.length).toBe(2);
        expect(names[0].textContent).toBe('Alice');
        expect(names[1].textContent).toBe('Bob');
    });

    it('author + date → two bare-div cells, date as <p class="date">', () => {
        const { container } = mount(
            derived({ title: ms('Doc'), date: ms('2026-05-10') }, ['Jane']),
        );
        const grid = container.querySelector('div.quarto-title-meta');
        const cells = grid!.querySelectorAll(':scope > div');
        expect(cells.length).toBe(2);
        expect(
            cells[1].querySelector('.quarto-title-meta-heading')!.textContent,
        ).toBe('Published');
        const date = cells[1].querySelector(
            '.quarto-title-meta-contents > p.date',
        );
        expect(date).not.toBeNull();
        expect(date!.textContent).toBe('2026-05-10');
    });

    it('date but NO author → date cell renders (Q1 parity; old quirk removed)', () => {
        const { container } = mount(
            derived({ title: ms('Doc'), date: ms('2026-05-10') }),
        );
        const grid = container.querySelector('div.quarto-title-meta');
        expect(grid).not.toBeNull();
        const date = grid!.querySelector('p.date');
        expect(date).not.toBeNull();
        expect(date!.textContent).toBe('2026-05-10');
    });

    it('abstract → outer div > div.abstract with block-title heading and <p>', () => {
        const { container } = mount(
            derived({ title: ms('Doc'), abstract: ms('A short summary.') }),
        );
        const abstract = container.querySelector(
            'header#title-block-header > div > div.abstract',
        );
        expect(abstract).not.toBeNull();
        expect(
            abstract!.querySelector(':scope > div.block-title')!.textContent,
        ).toBe('Abstract');
        expect(container.querySelector('.abstract-title')).toBeNull();
        const p = abstract!.querySelector(':scope > p');
        expect(p).not.toBeNull();
        expect(p!.textContent).toBe('A short summary.');
    });

    it('labels from meta override the hardcoded fallbacks', () => {
        const { container } = mount({
            title: ms('Doc'),
            date: ms('2026-05-10'),
            'by-author': byAuthor('Jane'),
            abstract: ms('S.'),
            labels: mm({
                authors: ms('Written by'),
                published: ms('Posted'),
                abstract: ms('Summary'),
            }),
            rendered: mm({ 'has-title-block': mb(true) }),
        });
        const headings = Array.from(
            container.querySelectorAll('.quarto-title-meta-heading'),
        ).map((el) => el.textContent);
        expect(headings).toEqual(['Written by', 'Posted']);
        expect(
            container.querySelector('div.block-title')!.textContent,
        ).toBe('Summary');
    });

    it('no title but authors present → header renders without <h1> (Q1 parity)', () => {
        const { container } = mount(
            derived({ date: ms('2026-05-10') }, ['Jane']),
        );
        const header = container.querySelector('header#title-block-header');
        expect(header).not.toBeNull();
        expect(header!.querySelector('h1.title')).toBeNull();
        expect(header!.querySelectorAll('p').length).toBeGreaterThan(0);
    });

    it('title with inline emphasis → renders as plain text (matches Rust)', () => {
        const { container } = mount(
            derived({
                title: mi(STR('Hello'), { t: 'Space' }, {
                    t: 'Emph',
                    c: [STR('World')],
                }),
            }),
        );
        const h1 = container.querySelector('h1.title');
        expect(h1).not.toBeNull();
        expect(h1!.textContent).toBe('Hello World');
        // No <em> inside the h1 — emphasis is stripped to match Rust.
        expect(h1!.querySelector('em')).toBeNull();
    });
});

describe('PreviewTitleBlock — Pandoc-falsy semantics', () => {
    it('empty-string title → header renders, <h1> suppressed', () => {
        const { container } = mount(derived({ title: ms('') }));
        expect(
            container.querySelector('header#title-block-header'),
        ).not.toBeNull();
        expect(container.querySelector('h1.title')).toBeNull();
    });

    it('title set + empty-string subtitle → <p class="subtitle"> is NOT rendered', () => {
        const { container } = mount(
            derived({ title: ms('Doc'), subtitle: ms('') }),
        );
        expect(container.querySelector('p.subtitle')).toBeNull();
    });

    it('no by-author → grid has no author cell', () => {
        const { container } = mount(derived({ title: ms('Doc') }));
        expect(
            container.querySelector('.quarto-title-meta-heading'),
        ).toBeNull();
    });

    it('title + author + empty-string date → no date cell', () => {
        const { container } = mount(
            derived({ title: ms('Doc'), date: ms('') }, ['Jane']),
        );
        const grid = container.querySelector('div.quarto-title-meta');
        expect(grid).not.toBeNull();
        expect(grid!.querySelector('p.date')).toBeNull();
    });

    it('title set + empty-string abstract → .abstract is NOT rendered', () => {
        const { container } = mount(
            derived({ title: ms('Doc'), abstract: ms('') }),
        );
        expect(container.querySelector('div.abstract')).toBeNull();
    });

    it('by-author entry with empty literal → dropped from the list', () => {
        const { container } = mount({
            title: ms('Doc'),
            'by-author': ml(
                mm({ name: mm({ literal: ms('Alice') }) }),
                mm({ name: mm({ literal: ms('') }) }),
            ),
            rendered: mm({ 'has-title-block': mb(true) }),
        });
        const names = container.querySelectorAll(
            '.quarto-title-meta-contents > p',
        );
        expect(names.length).toBe(1);
        expect(names[0].textContent).toBe('Alice');
    });
});

describe('PreviewTitleBlock — structured authors (P2, bd-ez0hiowa)', () => {
    /** One rich normalized by-author entry, as the P2 transform writes it. */
    const richAuthors = () =>
        ml(
            mm({
                name: mm({ literal: ms('Norah Jones') }),
                url: ms('https://example.com/norah'),
                email: ms('norah@example.com'),
                orcid: ms('0000-0002-1825-0097'),
                degrees: ml(ms('PhD')),
                affiliations: ml(
                    mm({
                        name: ms('Carnegie Mellon University'),
                        department: ms('School of Music'),
                    }),
                ),
            }),
            mm({
                name: mm({ literal: ms('Bill Malone') }),
                affiliations: ml(
                    mm({
                        name: ms('University of Texas'),
                        url: ms('https://utexas.edu'),
                    }),
                ),
            }),
        );

    function mountRich() {
        return mount({
            title: ms('Doc'),
            'by-author': richAuthors(),
            labels: mm({
                authors: ms('Authors'),
                affiliations: ms('Affiliations'),
            }),
            rendered: mm({ 'has-title-block': mb(true) }),
        });
    }

    it('affiliations present → two-column quarto-title-meta-author grid', () => {
        const { container } = mountRich();
        const grid = container.querySelector('div.quarto-title-meta-author');
        expect(grid).not.toBeNull();
        const headings = grid!.querySelectorAll(
            ':scope > div.quarto-title-meta-heading',
        );
        expect(Array.from(headings).map((h) => h.textContent)).toEqual([
            'Authors',
            'Affiliations',
        ]);
        // One author cell + one affiliation cell per author.
        const cells = grid!.querySelectorAll(
            ':scope > div.quarto-title-meta-contents',
        );
        expect(cells.length).toBe(4);
        expect(cells[0].querySelector('p.author')).not.toBeNull();
        expect(cells[1].querySelector('p.affiliation')).not.toBeNull();
        // The plain meta grid renders but carries no authors cell
        // (Q1's $if(by-affiliation)$ / $elseif(by-author)$ split).
        const plainGrid = container.querySelector('div.quarto-title-meta');
        expect(plainGrid).not.toBeNull();
        expect(plainGrid!.children.length).toBe(0);
    });

    it('author name links to url with degrees inside the anchor', () => {
        const { container } = mountRich();
        const link = container.querySelector(
            'p.author > a[href="https://example.com/norah"]',
        );
        expect(link).not.toBeNull();
        expect(link!.textContent).toBe('Norah Jones, PhD');
    });

    it('email → quarto-title-author-email anchor with inline SVG', () => {
        const { container } = mountRich();
        const email = container.querySelector(
            'p.author > a.quarto-title-author-email[href="mailto:norah@example.com"]',
        );
        expect(email).not.toBeNull();
        expect(email!.querySelector('svg')).not.toBeNull();
    });

    it('orcid → quarto-title-author-orcid anchor with inline SVG', () => {
        const { container } = mountRich();
        const orcid = container.querySelector(
            'p.author > a.quarto-title-author-orcid[href="https://orcid.org/0000-0002-1825-0097"]',
        );
        expect(orcid).not.toBeNull();
        expect(orcid!.getAttribute('aria-label')).toBe(
            'ORCID profile for Norah Jones',
        );
        expect(orcid!.querySelector('svg')).not.toBeNull();
    });

    it('affiliation with url renders as a link', () => {
        const { container } = mountRich();
        const affLink = container.querySelector(
            'p.affiliation > a[href="https://utexas.edu"]',
        );
        expect(affLink).not.toBeNull();
        expect(affLink!.textContent).toBe('University of Texas');
    });

    it('no affiliations → single-column path, decorations still render', () => {
        const { container } = mount({
            title: ms('Doc'),
            'by-author': ml(
                mm({
                    name: mm({ literal: ms('Jane Doe') }),
                    orcid: ms('0000-0001-0000-0000'),
                }),
            ),
            rendered: mm({ 'has-title-block': mb(true) }),
        });
        expect(
            container.querySelector('div.quarto-title-meta-author'),
        ).toBeNull();
        const cell = container.querySelector(
            'div.quarto-title-meta .quarto-title-meta-contents > p',
        );
        expect(cell).not.toBeNull();
        expect(
            cell!.querySelector('a.quarto-title-author-orcid'),
        ).not.toBeNull();
    });

    it('multi-paragraph abstract → one <p> per paragraph (P2 fidelity fix)', () => {
        const { container } = mount(
            derived({
                title: ms('Doc'),
                abstract: {
                    t: 'MetaBlocks',
                    c: [PARA(STR('First.')), PARA(STR('Second.'))],
                },
            }),
        );
        const abstract = container.querySelector('div.abstract');
        expect(abstract).not.toBeNull();
        const paras = abstract!.querySelectorAll(':scope > p');
        expect(paras.length).toBe(2);
        expect(paras[0].textContent).toBe('First.');
        expect(paras[1].textContent).toBe('Second.');
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
            derived({ title: ms('Doc') }),
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
            derived({ title: ms('Doc') }),
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

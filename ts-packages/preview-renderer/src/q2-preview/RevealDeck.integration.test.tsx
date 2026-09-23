/**
 * RevealDeck deck-chrome parity (bd-n2w0sxgd, F3 of the
 * 2026-06-17 preview↔render parity audit).
 *
 * `q2 render` places the deck-level footer + logo as DIRECT children of
 * `.reveal`, OUTSIDE `.slides`, and adds `has-logo` to `.reveal`
 * (crates/quarto-core/src/revealjs/assemble.rs::render_revealjs_document +
 * footer_logo_html). `q2 preview`'s `RevealDeck` must mirror that. The markup
 * is pre-rendered by the `reveal-footer-logo` transform into the
 * `rendered.reveal.{footer,logo}` meta slots (which run in the q2-preview
 * pipeline), so the React side only reads + places it.
 *
 * `<Deck>` from `@revealjs/react` renders children only inside `.slides`, so the
 * chrome is injected into the `.reveal` element imperatively via `useReveal()` →
 * `getRevealElement()` (the `RevealChrome` component). These tests drive that
 * component directly with a fake `RevealContext`, so they assert the exact DOM
 * mutation without booting reveal.js.
 */

import { describe, it, expect } from 'vitest';
import { isValidElement } from 'react';
import { render } from '@testing-library/react';
import { RevealContext, Slide, Stack } from '@revealjs/react';
import {
    RevealChrome,
    RevealNavSync,
    revealChromeFromMeta,
    renderTopSection,
    sectionAttrProps,
} from './RevealDeck';

const metaString = (c: string) => ({ t: 'MetaString', c });
const metaMap = (entries: Record<string, unknown>) => ({
    t: 'MetaMap',
    c: entries,
});

/** Top-level meta carrying `rendered.reveal.{footer,logo}` slots. */
function metaWithReveal(slots: { footer?: string; logo?: string }) {
    const reveal: Record<string, unknown> = {};
    if (slots.footer !== undefined) reveal.footer = metaString(slots.footer);
    if (slots.logo !== undefined) reveal.logo = metaString(slots.logo);
    return { rendered: metaMap({ reveal: metaMap(reveal) }) };
}

const FOOTER_HTML =
    '<div class="footer footer-default">© 2026 Example Corp — <a href="https://example.com">example.com</a></div>';
const LOGO_HTML = '<img class="slide-logo" src="logo.svg">';

/** Render `RevealChrome` against a real `.reveal`/`.slides` element via a fake
 *  reveal API, returning that `.reveal` element for assertions. */
function renderChrome(props: { footerHtml?: string; logoHtml?: string }) {
    const reveal = document.createElement('div');
    reveal.className = 'reveal';
    const slides = document.createElement('div');
    slides.className = 'slides';
    reveal.appendChild(slides);
    document.body.appendChild(reveal);
    const fakeApi = { getRevealElement: () => reveal } as never;
    const utils = render(
        <RevealContext.Provider value={fakeApi}>
            <RevealChrome {...props} />
        </RevealContext.Provider>,
    );
    return { reveal, slides, ...utils };
}

/** A `Div(.section)` block as `RevealSlidesTransform` emits it: Attr =
 *  [id, classes, kvs], then the child blocks. */
const sectionDiv = (
    id: string,
    classes: string[],
    blocks: unknown[] = [],
): never =>
    ({ t: 'Div', c: [[id, classes, []], blocks] }) as never;

const H1_TITLE = { t: 'Header', c: [1, ['', ['title'], []], [{ t: 'Str', c: 'T' }]] };

describe('sectionAttrProps — forwards the section Div Attr (F1+F2, bd-vv8jft5n)', () => {
    it('joins classes and keeps the id', () => {
        const props = sectionAttrProps(
            sectionDiv('title-slide', ['section', 'title-slide', 'center']),
        );
        expect(props.id).toBe('title-slide');
        expect(props.className).toBe('section title-slide center');
    });

    it('omits an empty id and empty className (untitled / class-less slide)', () => {
        const props = sectionAttrProps(sectionDiv('', []));
        expect(props.id).toBeUndefined();
        expect(props.className).toBeUndefined();
    });
});

describe('renderTopSection — title slide keeps `center` so reveal centers it', () => {
    it('leaf <Slide> carries the section id + classes (title-slide center)', () => {
        const div = sectionDiv('title-slide', ['section', 'title-slide', 'center'], [
            H1_TITLE,
        ]);
        const el = renderTopSection(div, 0);
        expect(isValidElement(el)).toBe(true);
        const node = el as React.ReactElement<Record<string, unknown>>;
        expect(node.type).toBe(Slide);
        expect(node.props.id).toBe('title-slide');
        expect(node.props.className).toBe('section title-slide center');
    });

    it('<Stack> divider carries `section` class; inner <Slide>s carry their id+classes', () => {
        const inner1 = sectionDiv('part-one', ['section']); // divider heading slide
        const inner2 = sectionDiv('first-topic', ['section']);
        const div = sectionDiv('part-one', ['section'], [inner1, inner2]);
        const el = renderTopSection(div, 0) as React.ReactElement<Record<string, unknown>>;
        expect(el.type).toBe(Stack);
        expect(el.props.className).toContain('section');
        const slides = el.props.children as React.ReactElement<Record<string, unknown>>[];
        expect(slides[0].props.id).toBe('part-one');
        expect(slides[1].props.id).toBe('first-topic');
        expect(slides[1].props.className).toBe('section');
    });
});

describe('revealChromeFromMeta — reads rendered.reveal.{footer,logo}', () => {
    it('extracts both slots', () => {
        const chrome = revealChromeFromMeta(
            metaWithReveal({ footer: FOOTER_HTML, logo: LOGO_HTML }) as never,
        );
        expect(chrome.footerHtml).toBe(FOOTER_HTML);
        expect(chrome.logoHtml).toBe(LOGO_HTML);
    });

    it('returns undefined slots when absent', () => {
        const chrome = revealChromeFromMeta({} as never);
        expect(chrome.footerHtml).toBeUndefined();
        expect(chrome.logoHtml).toBeUndefined();
    });
});

describe('RevealChrome — places footer/logo outside .slides, adds has-logo', () => {
    it('injects logo + footer as direct children of .reveal (outside .slides)', () => {
        const { reveal, slides } = renderChrome({
            footerHtml: FOOTER_HTML,
            logoHtml: LOGO_HTML,
        });

        const logo = reveal.querySelector('img.slide-logo');
        const footer = reveal.querySelector('div.footer.footer-default');
        expect(logo).not.toBeNull();
        expect(footer).not.toBeNull();
        // Direct children of `.reveal`, NOT nested in `.slides`.
        expect(logo!.parentElement).toBe(reveal);
        expect(footer!.parentElement).toBe(reveal);
        expect(slides.contains(logo!)).toBe(false);
        expect(slides.contains(footer!)).toBe(false);
        // Footer preserves inline markup (link survives).
        expect(footer!.querySelector('a')?.getAttribute('href')).toBe(
            'https://example.com',
        );
    });

    it('orders logo before footer (matches assemble.rs footer_logo_html)', () => {
        const { reveal } = renderChrome({ footerHtml: FOOTER_HTML, logoHtml: LOGO_HTML });
        const logo = reveal.querySelector('img.slide-logo')!;
        const footer = reveal.querySelector('div.footer')!;
        // logo precedes footer in document order
        expect(
            logo.compareDocumentPosition(footer) &
                Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
    });

    it('adds has-logo to .reveal only when a logo is present', () => {
        const withLogo = renderChrome({ footerHtml: FOOTER_HTML, logoHtml: LOGO_HTML });
        expect(withLogo.reveal.classList.contains('has-logo')).toBe(true);

        const footerOnly = renderChrome({ footerHtml: FOOTER_HTML });
        expect(footerOnly.reveal.classList.contains('has-logo')).toBe(false);
        expect(footerOnly.reveal.querySelector('div.footer')).not.toBeNull();
        expect(footerOnly.reveal.querySelector('img.slide-logo')).toBeNull();
    });

    it('injects nothing when both slots are absent', () => {
        const { reveal, slides } = renderChrome({});
        expect(reveal.children.length).toBe(1); // only `.slides`
        expect(reveal.firstElementChild).toBe(slides);
        expect(reveal.classList.contains('has-logo')).toBe(false);
    });

    it('removes injected chrome + has-logo on unmount', () => {
        const { reveal, unmount } = renderChrome({
            footerHtml: FOOTER_HTML,
            logoHtml: LOGO_HTML,
        });
        expect(reveal.querySelector('.slide-logo')).not.toBeNull();
        unmount();
        expect(reveal.querySelector('.slide-logo')).toBeNull();
        expect(reveal.querySelector('.footer')).toBeNull();
        expect(reveal.classList.contains('has-logo')).toBe(false);
    });
});

/** Fake reveal API exposing only the slice `RevealNavSync` uses, plus an
 *  `emit` test hook to fire a registered event. `slideCalls` records every
 *  imperative `slide(n)`; `state.h` tracks the current horizontal index. */
function makeFakeReveal(initialH = 0) {
    const listeners: Record<string, Array<() => void>> = {};
    const state = { h: initialH };
    const slideCalls: number[] = [];
    const api = {
        getIndices: () => ({ h: state.h }),
        slide: (n: number) => {
            slideCalls.push(n);
            state.h = n;
        },
        on: (t: string, cb: () => void) => {
            (listeners[t] ??= []).push(cb);
        },
        off: (t: string, cb: () => void) => {
            listeners[t] = (listeners[t] ?? []).filter((c) => c !== cb);
        },
        emit: (t: string) => (listeners[t] ?? []).forEach((c) => c()),
    };
    return { api, state, slideCalls };
}

describe('RevealNavSync — cursor↔slide bridge (bd-mwbsdmel)', () => {
    it('registers a goTo navigator that drives reveal.slide, no-op when already there', () => {
        const fake = makeFakeReveal(0);
        let nav: ((i: number) => void) | null = null;
        render(
            <RevealContext.Provider value={fake.api as never}>
                <RevealNavSync registerSlideNavigator={(n) => { nav = n; }} />
            </RevealContext.Provider>,
        );
        expect(typeof nav).toBe('function');

        nav!(2);
        expect(fake.slideCalls).toEqual([2]);
        expect(fake.state.h).toBe(2);

        // Already on slide 2 → no redundant slide() (so a host echo of the
        // deck's own position can't fight a user mid-navigation).
        nav!(2);
        expect(fake.slideCalls).toEqual([2]);
    });

    it('reports in-deck navigation via onSlideChange with the horizontal index', () => {
        const fake = makeFakeReveal(0);
        const changes: number[] = [];
        render(
            <RevealContext.Provider value={fake.api as never}>
                <RevealNavSync onSlideChange={(i) => changes.push(i)} />
            </RevealContext.Provider>,
        );
        fake.state.h = 3;
        fake.api.emit('slidechanged');
        expect(changes).toEqual([3]);
    });

    it('clears the registered navigator on unmount', () => {
        const fake = makeFakeReveal(0);
        const registered: Array<((i: number) => void) | null> = [];
        const { unmount } = render(
            <RevealContext.Provider value={fake.api as never}>
                <RevealNavSync registerSlideNavigator={(n) => { registered.push(n); }} />
            </RevealContext.Provider>,
        );
        expect(typeof registered.at(-1)).toBe('function');
        unmount();
        expect(registered.at(-1)).toBeNull();
    });
});

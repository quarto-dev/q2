/**
 * Iframe-side theme-on-slides behaviour (bd-y259zb57 parity).
 *
 * Unlike `entry.integration.test.tsx` (which mocks `createRoot` and only
 * exercises the module-top `UPDATE_THEME` → `<link data-q2-theme>` path for a
 * NON-slide doc), this test drives the REAL render path: a slides `UPDATE_AST`
 * mounts the real `PreviewRoot` (so the active doc genuinely IS a reveal deck),
 * then an `UPDATE_THEME` arrives. It pins the contract that the compiled theme
 * link IS applied on a reveal deck, matching `q2 render`.
 *
 * Fail-on-revert: re-introducing the old slide-theme suppression (a
 * `currentDocIsSlides` gate making `reconcileThemeLink()` call `applyTheme(null)`
 * on slides) yields no `<link data-q2-theme>` → this test goes RED.
 */

// @vitest-environment jsdom

import { describe, it, expect, beforeAll, afterEach, vi } from 'vitest';
import { act } from '@testing-library/react';

// RevealDeck (rendered when the AST is a slide deck) imports @revealjs/react
// and the vendored reveal CSS — stub them so mounting is light in jsdom. Do
// NOT mock react-dom/client: we need the real render so the active doc is a
// genuine reveal deck when UPDATE_THEME arrives.
vi.mock('@revealjs/react', () => ({ Deck: () => null, Slide: () => null }));
vi.mock('../../../../resources/revealjs/reset.css', () => ({}));
vi.mock('../../../../resources/revealjs/reveal.css', () => ({}));
vi.mock('../../../../resources/revealjs/theme/white.css', () => ({}));
vi.mock('../../../../resources/revealjs/quarto-reveal.css', () => ({}));
vi.mock('katex/dist/katex.min.css', () => ({}));

beforeAll(async () => {
    // Side-effect import: registers the module-top message listener.
    await import('./entry');
});

afterEach(() => {
    document.head
        .querySelectorAll('link[data-q2-theme]')
        .forEach((el) => el.remove());
});

// A minimal slide-deck AST: meta.format = "revealjs" makes PreviewRoot's
// `isSlides` true (extractMetaString reads MetaString.c), so the mounted doc
// is a reveal deck.
const SLIDES_AST = JSON.stringify({
    'pandoc-api-version': [1, 23, 0],
    meta: { format: { t: 'MetaString', c: 'revealjs' } },
    blocks: [],
});

async function postMessageAndFlush(data: unknown) {
    await act(async () => {
        window.dispatchEvent(new MessageEvent('message', { data }));
        await Promise.resolve();
    });
}

function themeLinks(): NodeListOf<HTMLLinkElement> {
    return document.head.querySelectorAll('link[data-q2-theme]');
}

describe('q2-preview/entry theme link on a slide deck', () => {
    it('applies the theme link on a slide deck (no suppression)', async () => {
        // jsdom has no #root by default; updateAst needs it to mount.
        const root = document.createElement('div');
        root.id = 'root';
        document.body.appendChild(root);
        try {
            // 1. Slides AST → real PreviewRoot mounts → isSlides effect →
            //    entry.setDocIsSlides(true).
            await postMessageAndFlush({ type: 'UPDATE_AST', payload: { astJson: SLIDES_AST, currentFilePath: 'deck.qmd' } });
            // 2. Theme arrives for the (slide) document.
            await postMessageAndFlush({ type: 'UPDATE_THEME', cssUrl: 'blob:slide-theme' });

            // The compiled theme MUST reach the deck (bd-y259zb57): one link
            // carrying the url. (Pre-fix: suppressed on slides → zero links.)
            const links = themeLinks();
            expect(links).toHaveLength(1);
            expect(links[0].getAttribute('href')).toBe('blob:slide-theme');
        } finally {
            root.remove();
        }
    });
});

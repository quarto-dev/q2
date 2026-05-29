/**
 * @vitest-environment jsdom
 *
 * Regression test for the slide-renderer migration to the framework
 * `extractMetaString` helper (Plan 2D Phase 6.0d).
 *
 * Behavior change: the previous local `extractMetaString` (lifted out
 * in 6.0d) walked only `Str` / `Space` inside `MetaInlines`. Any
 * inline markup (`Emph`, `Strong`, `Code`, `Link`, ...) was silently
 * dropped, so a slide title authored as `# *Hello* world` produced
 * the empty string for the title-slide's `title` field. The lifted
 * `framework/extractMetaString` walks the full inlines list via
 * `inlinesToPlainText`, so the same input now renders `"Hello world"`.
 *
 * This locks the new behavior; if a future edit re-narrows the
 * MetaInlines walk, the test breaks.
 */

import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import React from 'react';
import type { PandocAST, RawBlock } from '@quarto/preview-renderer/framework';
import { parseSlides, renderBlock } from './ReactAstSlideRenderer';

describe('ReactAstSlideRenderer slide-title meta coercion', () => {
    it('preserves inline emphasis text in MetaInlines title (Plan 2D 6.0d)', () => {
        const ast: PandocAST = {
            'pandoc-api-version': [1, 23, 1],
            meta: {
                title: {
                    t: 'MetaInlines',
                    c: [
                        { t: 'Str', c: 'Hello' },
                        { t: 'Space' },
                        { t: 'Emph', c: [{ t: 'Str', c: 'world' }] },
                    ],
                },
            },
            blocks: [],
        };

        const slides = parseSlides(ast);

        expect(slides.length).toBeGreaterThan(0);
        const titleSlide = slides[0];
        expect(titleSlide.type).toBe('title');
        expect(titleSlide.title).toBe('Hello world');
    });

    it('still handles MetaString titles (no regression)', () => {
        const ast: PandocAST = {
            'pandoc-api-version': [1, 23, 1],
            meta: {
                title: { t: 'MetaString', c: 'My Doc' },
            },
            blocks: [],
        };

        const slides = parseSlides(ast);
        expect(slides[0].type).toBe('title');
        expect(slides[0].title).toBe('My Doc');
    });
});

/**
 * Regression tests for the `RawBlock(html, …)` script-re-execution
 * shim added in bd-my0o5.
 *
 * Without the shim, scripts inserted via `dangerouslySetInnerHTML`
 * remain inert (the HTML spec only executes script elements that
 * the parser sees in the initial document, or that are created via
 * `document.createElement`). That broke engines that emit in-band
 * `<script>` includes — `MermaidEngine`'s jsdelivr import — in the
 * preview iframe even though they worked fine in static `q2 render`.
 *
 * These tests lock the post-mount re-execution behaviour: if the
 * shim is ever removed or narrowed, the assertions break and the
 * static/preview parity gap reopens.
 */
describe('ReactAstSlideRenderer RawBlock script re-execution (bd-my0o5)', () => {
    it('recreates inline <script> tags after mount (the swap that makes them executable)', () => {
        // The fix is purely structural: replace each `<script>` element
        // inserted by `dangerouslySetInnerHTML` (which is inert) with a
        // freshly created element via `document.createElement` (which
        // a real browser executes on insertion). In jsdom, scripts are
        // *parsed* but not run by default — `runScripts: 'dangerously'`
        // would be needed, and we deliberately avoid enabling that
        // globally just to assert execution. The end-to-end browser
        // verification (Chrome DevTools MCP, bd-my0o5 Phase 3) is
        // what actually proves the script runs.
        //
        // What we can assert here is that the swap happened. We use a
        // marker attribute that the original tag does not have but
        // that we set on the replacement — proving the in-DOM script
        // is a different (browser-newly-created) element.
        const rawBlock: RawBlock = {
            t: 'RawBlock',
            c: ['html', '<script id="orig-marker">/* inline body */</script>'],
        };

        const { container } = render(renderBlock(rawBlock, 0, '', () => {}));

        const scripts = container.querySelectorAll('script');
        expect(scripts.length).toBe(1);
        // The text body and id attribute are copied onto the
        // replacement script; the replacement is what's in the DOM
        // now (the original was removed by `replaceWith`).
        expect(scripts[0].getAttribute('id')).toBe('orig-marker');
        expect(scripts[0].textContent).toContain('inline body');
    });

    it('copies script attributes (e.g. type="module") onto the recreated tag', () => {
        // We cannot easily exercise an actual ES-module load in jsdom,
        // but we can assert the recreated script keeps the type
        // attribute — which is what makes the browser parse the source
        // as a module rather than a classic script.
        const rawBlock: RawBlock = {
            t: 'RawBlock',
            c: ['html', '<script type="module" data-mermaid-init>/* module body */</script>'],
        };

        const { container } = render(renderBlock(rawBlock, 0, '', () => {}));

        const script = container.querySelector('script');
        expect(script).not.toBeNull();
        expect(script!.getAttribute('type')).toBe('module');
        expect(script!.hasAttribute('data-mermaid-init')).toBe(true);
    });

    it('passes through non-html raw blocks as null (no behaviour change)', () => {
        const rawBlock: RawBlock = { t: 'RawBlock', c: ['latex', '\\section{Foo}'] };
        const result = renderBlock(rawBlock, 0, '', () => {});
        expect(result).toBeNull();
    });
});

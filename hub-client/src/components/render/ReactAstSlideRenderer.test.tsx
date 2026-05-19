/**
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
import type { PandocAST } from '@quarto/preview-renderer/framework';
import { parseSlides } from './ReactAstSlideRenderer';

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

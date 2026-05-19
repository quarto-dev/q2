/**
 * Phase D contract tests for the shared attribution viewer CSS.
 *
 * The CLI's `AttributionViewerTransform` and the hub-client's
 * `framework/attribution.tsx` both consume the same
 * `resources/attribution/viewer.css` — the Rust side via
 * `include_str!`, the hub-client side via Vite's `?raw`. Drift between
 * the two would silently break visual presentation on one surface.
 *
 * These tests pin the shared-asset contract:
 * - `attributionStyles` is exactly the contents of `viewer.css?raw`.
 * - The shared file mentions the badge class names the framework
 *   widget renders against.
 *
 * @vitest-environment jsdom
 */

import { describe, expect, it } from 'vitest';

import { attributionStyles } from './attribution';
import viewerCss from 'virtual:quarto-attribution-viewer-css';

describe('attributionStyles', () => {
    it('re-exports the shared viewer.css verbatim', () => {
        expect(attributionStyles).toBe(viewerCss);
    });

    it('contains the badge class names the framework renders against', () => {
        for (const cls of ['q2-attr-badge', 'q2-attr-badge-dot', 'q2-attr-badge-time']) {
            expect(attributionStyles).toContain(cls);
        }
    });
});

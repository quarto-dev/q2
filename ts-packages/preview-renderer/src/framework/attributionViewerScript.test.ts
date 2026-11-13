/**
 * @vitest-environment jsdom
 */

import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const viewerScript = readFileSync('../../resources/attribution/viewer.js', 'utf8');

describe('attribution viewer script', () => {
    it('keeps a long-name badge inside the viewport near the right edge', () => {
        Object.defineProperty(document.documentElement, 'clientWidth', {
            configurable: true,
            value: 1280,
        });
        document.body.innerHTML = `
            <span id="leaf" data-attr-actor="alice" data-attr-time="0"
                style="--attr-color: #CC6677; --attr-name: '${'A'.repeat(100)}'">text</span>
        `;
        const leaf = document.getElementById('leaf')!;
        leaf.getBoundingClientRect = () =>
            ({ left: 1200, right: 1210, top: 10, bottom: 20, width: 10, height: 10 }) as DOMRect;

        window.eval(viewerScript);
        leaf.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));

        const badge = document.querySelector<HTMLElement>('.q2-attr-badge')!;
        expect(badge.textContent).toContain('A'.repeat(100));
        // Anchored to the leaf's right edge, so a short badge stays adjacent.
        expect(badge.style.left).toBe('');
        expect(badge.style.right).toBe('70px');
        expect(badge.style.maxWidth).toBe('320px');
    });
});

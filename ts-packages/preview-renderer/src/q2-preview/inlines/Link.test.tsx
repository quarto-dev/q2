// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { Link } from './Link';
import type { LinkInline, NodeArgs } from '../../framework';

const renderLink = (kvs: [string, string][]) => {
    const node = {
        t: 'Link',
        c: [['', [], kvs], [{ t: 'Str', c: 'Doe 2020' }], ['#ref-doe2020', '']],
    } as unknown as LinkInline;
    const { container } = render(<Link {...({ node } as NodeArgs<LinkInline>)} />);
    return container.querySelector('a')!;
};

describe('Link attributes', () => {
    // Citation links carry the DPUB-ARIA `role="doc-biblioref"`, as the
    // HTML writer renders them.
    it('keeps role', () => {
        const a = renderLink([['role', 'doc-biblioref']]);
        expect(a.getAttribute('role')).toBe('doc-biblioref');
        expect(a.getAttribute('href')).toBe('#ref-doe2020');
    });

    it('drops keys that are not passed through', () => {
        const a = renderLink([['custom', 'y']]);
        expect(a.getAttribute('custom')).toBeNull();
    });
});

describe('Lightbox links', () => {
    const renderLightbox = (classes: string[]) => {
        const node = {
            t: 'Link',
            c: [
                ['', classes, [['data-gallery', 'g']]],
                [{ t: 'Image', c: [['', [], []], [], ['img.png', '']] }],
                ['img.png', 'A title'],
            ],
        } as unknown as LinkInline;
        const { container } = render(<Link {...({ node } as NodeArgs<LinkInline>)} />);
        return container.querySelector('a')!;
    };

    // bd-kcpkgftw: the preview has no GLightbox, so a click must not
    // navigate the iframe to the raw image; the markup stays as rendered.
    it('swallows the click but keeps the render markup', () => {
        const a = renderLightbox(['lightbox']);
        expect(a.getAttribute('href')).toBe('img.png');
        expect(a.getAttribute('data-gallery')).toBe('g');
        expect(a.getAttribute('title')).toBe('A title');
        const event = new MouseEvent('click', { bubbles: true, cancelable: true });
        a.dispatchEvent(event);
        expect(event.defaultPrevented).toBe(true);
    });

    it('leaves other links clickable', () => {
        const a = renderLightbox([]);
        // Record what React left, then stop jsdom's (unimplemented)
        // navigation. The document listener runs after React's root one.
        let preventedByLink: boolean | undefined;
        document.addEventListener(
            'click',
            (e) => {
                preventedByLink = e.defaultPrevented;
                e.preventDefault();
            },
            { once: true },
        );
        a.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
        expect(preventedByLink).toBe(false);
    });
});

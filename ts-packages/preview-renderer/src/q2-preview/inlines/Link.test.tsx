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

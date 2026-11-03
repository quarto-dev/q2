// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { Cite } from './Cite';
import type { CiteInline, NodeArgs } from '../../framework';

const citation = (id: string) => ({ citationId: id, citationMode: { t: 'NormalCitation' } });

// No children: rendering them needs the inline wrapper registry (see
// Span.test.tsx); the smoke-all E2E fixture covers the rendered text.
const renderCite = (ids: string[]) => {
    const node = {
        t: 'Cite',
        c: [ids.map(citation), []],
    } as unknown as CiteInline;
    const { container } = render(<Cite {...({ node } as NodeArgs<CiteInline>)} />);
    return container;
};

describe('Cite', () => {
    // Parity with the HTML writer (GH #1026): a citation renders inside
    // `<span class="citation" data-cites="...">`, as Pandoc writes it.
    it('wraps the citation in span.citation with data-cites', () => {
        const container = renderCite(['knuth1984', 'lamport1994']);
        const span = container.querySelector('span.citation')!;
        expect(span).not.toBeNull();
        expect(span.getAttribute('data-cites')).toBe('knuth1984 lamport1994');
    });
});

// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { Span } from './Span';
import type { NodeArgs, SpanInline } from '../../framework';

const renderSpan = (classes: string[], kvs: [string, string][]) => {
    const node = { t: 'Span', c: [['', classes, kvs], []] } as SpanInline;
    const { container } = render(<Span {...({ node } as NodeArgs<SpanInline>)} />);
    return container.querySelector('span')!;
};

describe('Span attributes', () => {
    it('keeps aria-hidden (link-external icon span)', () => {
        const span = renderSpan(
            ['bi', 'bi-box-arrow-up-right', 'quarto-ext-icon'],
            [['aria-hidden', 'true']],
        );
        expect(span.getAttribute('aria-hidden')).toBe('true');
        expect(span.className).toBe('bi bi-box-arrow-up-right quarto-ext-icon');
    });

    it('keeps data- attributes and drops other keys', () => {
        const span = renderSpan([], [['data-x', '1'], ['custom', 'y']]);
        expect(span.getAttribute('data-x')).toBe('1');
        expect(span.getAttribute('custom')).toBeNull();
    });
});

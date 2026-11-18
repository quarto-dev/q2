// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { Image } from './Image';
import type { NodeArgs, ImageInline } from '../../framework';

const renderImage = (kvs: [string, string][]) => {
    const node = { t: 'Image', c: [['', [], kvs], [], ['pic.png', '']] } as ImageInline;
    const { container } = render(<Image {...({ node } as NodeArgs<ImageInline>)} />);
    return container.querySelector('img')!;
};

describe('Image dimensions', () => {
    it('keeps unitless numbers as HTML attributes', () => {
        const img = renderImage([['width', '300'], ['height', '200.5']]);
        expect(img.getAttribute('width')).toBe('300');
        expect(img.getAttribute('height')).toBe('200.5');
        expect(img.getAttribute('style')).toBeNull();
    });

    it('puts unit-bearing values in style, not attributes (docx import emits inches)', () => {
        const img = renderImage([['width', '3.8333333333333335in'], ['height', '2.2916666666666665in']]);
        expect(img.getAttribute('width')).toBeNull();
        expect(img.getAttribute('height')).toBeNull();
        expect(img.style.width).toBe('3.8333333333333335in');
        expect(img.style.height).toBe('2.2916666666666665in');
    });

    it('handles percentages and mixed forms', () => {
        const img = renderImage([['width', '50%'], ['height', '120']]);
        expect(img.style.width).toBe('50%');
        expect(img.getAttribute('height')).toBe('120');
    });

    it('applies the style key-value the image-dimensions transform writes', () => {
        const img = renderImage([['height', '120'], ['style', 'width:60%']]);
        expect(img.style.width).toBe('60%');
        expect(img.getAttribute('height')).toBe('120');
        expect(img.getAttribute('width')).toBeNull();
    });

    it("keeps the author's own style declarations alongside lifted ones", () => {
        const img = renderImage([['style', 'border: 1px solid red;width:50vw;height:3in']]);
        expect(img.style.border).toBe('1px solid red');
        expect(img.style.width).toBe('50vw');
        expect(img.style.height).toBe('3in');
    });

    it('lets a unit-bearing width key-value win over style', () => {
        const img = renderImage([['style', 'width:10%'], ['width', '40%']]);
        expect(img.style.width).toBe('40%');
    });
});

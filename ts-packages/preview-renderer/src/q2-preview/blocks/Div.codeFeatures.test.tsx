// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { Div } from './Div';
import type { DivBlock, NodeArgs } from '../../framework';

/**
 * The preview forms of the code fold and the annotation list. The native
 * writer gets both as split raw HTML; the transforms send the preview Divs
 * (`code_block_render.rs::wrap_with_fold_div`,
 * `code_annotations.rs::definition_list_divs`) and `Div` renders the same
 * elements.
 */

function renderDiv(classes: string[], kvs: [string, string][], content: unknown[] = []) {
    const node = { t: 'Div', c: [['', classes, kvs], content] } as unknown as DivBlock;
    const args = { node, setLocalAst: () => {} } as NodeArgs<DivBlock>;
    return render(<Div {...args} />).container;
}

describe('Div code fold', () => {
    it('renders a closed details with the summary text', () => {
        const c = renderDiv(['code-fold'], [['data-code-summary', 'Show the sum']]);
        const details = c.querySelector('details')!;
        expect(details.className).toBe('code-fold');
        expect(details.open).toBe(false);
        expect(details.querySelector(':scope > summary')!.textContent).toBe('Show the sum');
    });

    it('is open for code-fold: show and keeps the hidden class', () => {
        const c = renderDiv(
            ['code-fold', 'hidden'],
            [['data-code-summary', 'Code'], ['data-code-fold-open', 'true']],
        );
        const details = c.querySelector('details')!;
        expect(details.open).toBe(true);
        expect(details.className).toBe('code-fold hidden');
    });

    it('leaves a user Div that merely has the class alone', () => {
        const c = renderDiv(['code-fold'], []);
        expect(c.querySelector('details')).toBeNull();
        expect(c.querySelector('div.code-fold')).not.toBeNull();
    });
});

describe('Div annotation list', () => {
    it('renders the container as dl and an item as dt + dd', () => {
        const list = renderDiv(['code-annotation-container-grid'], []);
        expect(list.firstElementChild!.tagName).toBe('DL');
        expect(list.firstElementChild!.className).toBe('code-annotation-container-grid');

        const item = renderDiv(
            ['code-annotation-item'],
            [['data-target-cell', 'annotated-cell-1'], ['data-target-annotation', '2']],
        );
        const dt = item.querySelector('dt')!;
        expect(dt.textContent).toBe('2');
        expect(dt.getAttribute('data-target-cell')).toBe('annotated-cell-1');
        expect(dt.getAttribute('data-target-annotation')).toBe('2');
        expect(dt.nextElementSibling!.tagName).toBe('DD');
    });
});

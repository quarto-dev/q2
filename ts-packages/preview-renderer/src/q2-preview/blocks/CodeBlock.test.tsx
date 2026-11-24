// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { CodeBlock } from './CodeBlock';
import type { CodeBlock as CodeBlockType, NodeArgs } from '../../framework';

/**
 * Line numbers and code annotations in the preview. The expected markup is
 * what the native writer emits (`write_highlighted_codeblock` in
 * `crates/pampa/src/writers/html.rs`); its tests there
 * (`number_lines_*`, `annotated_codeblock_*`) are the other half of this
 * contract.
 */

function renderCode(id: string, classes: string[], kvs: [string, string][], text: string) {
    const node = { t: 'CodeBlock', c: [[id, classes, kvs], text] } as unknown as CodeBlockType;
    const args = { node, setLocalAst: () => {} } as NodeArgs<CodeBlockType>;
    return render(<CodeBlock {...args} />).container;
}

describe('CodeBlock line numbers', () => {
    it('emits Pandoc per-line spans for a .number-lines block', () => {
        const c = renderCode('cb1', ['python', 'number-lines'], [['data-hl-spans', '[]']], 'a = 1\nb = 2');
        const pre = c.querySelector('pre')!;
        expect(pre.className).toBe('sourceCode numberSource python');
        expect(c.querySelector('div.sourceCode')!.className).toBe('sourceCode number-lines');
        const lines = [...c.querySelectorAll('code > span')];
        expect(lines.map((l) => l.id)).toEqual(['cb1-1', 'cb1-2']);
        expect(lines.map((l) => l.textContent)).toEqual(['a = 1', 'b = 2']);
        expect(lines[0].querySelector('a')!.getAttribute('href')).toBe('#cb1-1');
        // Lines are separated by the newline the source had.
        expect(c.querySelector('code')!.textContent).toBe('a = 1\nb = 2');
    });

    it('offsets ids and the counter for startFrom, and adds no line for a final newline', () => {
        const c = renderCode(
            'cb3',
            ['python', 'numberLines'],
            [['data-hl-spans', '[]'], ['startFrom', '10']],
            'a\nb\n',
        );
        const code = c.querySelector('code')!;
        expect(code.style.counterReset).toBe('source-line 9');
        expect([...code.querySelectorAll(':scope > span')].map((l) => l.id)).toEqual(['cb3-10', 'cb3-11']);
    });

    it('closes and reopens a highlight span that crosses a line break', () => {
        const c = renderCode(
            'cb1',
            ['python', 'number-lines'],
            [['data-hl-spans', '[[0,5,"string"]]']],
            '"a\nb"',
        );
        const lines = [...c.querySelectorAll('code > span')];
        expect(lines).toHaveLength(2);
        expect(lines[0].querySelector('span.hl-string')!.textContent).toBe('"a');
        expect(lines[1].querySelector('span.hl-string')!.textContent).toBe('b"');
    });

    it('does not treat a lone numbering class as the language', () => {
        const c = renderCode('cb1', ['number-lines'], [], 'x');
        expect(c.querySelector('pre')!.className).toBe('sourceCode numberSource');
        expect(c.querySelector('code')!.className).toBe('sourceCode');
    });

    it('leaves an unnumbered block without line spans', () => {
        const c = renderCode('', ['python'], [['data-hl-spans', '[]']], 'a\nb');
        expect(c.querySelector('code')!.children).toHaveLength(0);
        expect(c.querySelector('code')!.textContent).toBe('a\nb');
    });

    it('puts a reveal line spec on the outer div, where the plugin reads it', () => {
        const c = renderCode(
            'cb5',
            ['python', 'number-lines'],
            [['data-hl-spans', '[]'], ['code-line-numbers', '|1|2']],
            'a\nb',
        );
        expect(c.querySelector('div.sourceCode')!.getAttribute('data-code-line-numbers')).toBe('|1|2');
        expect(c.querySelector('pre')!.hasAttribute('data-code-line-numbers')).toBe(false);
    });
});

describe('CodeBlock annotations', () => {
    it('emits anchors, targets and the gutter, and hides the instruction attribute', () => {
        const c = renderCode(
            'annotated-cell-1',
            ['python', 'code-annotation-code'],
            [['data-hl-spans', '[]'], ['code-annotation-anchors', '2=1']],
            'a = 1\nb = 2',
        );
        const pre = c.querySelector('pre')!;
        expect(pre.className).toBe('sourceCode python code-annotation-code code-annotated');
        // The class moves from the div to the pre, as in the native writer.
        expect(c.querySelector('div.sourceCode')!.className).toBe('sourceCode');
        expect(c.innerHTML).not.toContain('code-annotation-anchors');

        const anchors = [...c.querySelectorAll('a.code-annotation-anchor')];
        expect(anchors).toHaveLength(1);
        expect(anchors[0].textContent).toBe('1');
        expect(anchors[0].getAttribute('data-target-cell')).toBe('annotated-cell-1');
        // The anchor is the sibling just before the line it marks.
        const target = c.querySelector('#annotated-cell-1-2')!;
        expect(target.className).toBe('code-annotation-target');
        expect(target.previousElementSibling).toBe(anchors[0]);
        expect(c.querySelector('#annotated-cell-1-1')!.className).toBe('');

        expect(c.querySelector('code > .code-annotation-gutter-bg')).not.toBeNull();
        expect(c.querySelector('code > .code-annotation-gutter')).not.toBeNull();
    });
});

/**
 * Task-list rendering + interactive toggle (bd-obkvhlam / bd-tvtknbhx).
 *
 * The reader parses `- [ ] todo` into Pandoc's convention: the item's first
 * inline is `Str "☐"`/`Str "☒"` followed by `Space`. The q2-preview renderer
 * must mirror the native HTML writer — `<ul class="task-list">` with
 * `<label><input type="checkbox" …/>` per item — and, when editing is
 * enabled, toggle a checkbox by flipping the ballot-box Str in the item's
 * untransformed source node and committing it through the subtree edit
 * channel (`apply_node_edit` splices `[ ]` ↔ `[x]` in the qmd source via the
 * qmd writer's task-marker round-trip).
 *
 * AST fixture generated with the real parser:
 *   `pampa <(printf -- '- [ ] todo\n- [x] done\n') -t json`
 */

import { describe, it, expect, afterEach, vi } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
});

const CONTENT = '- [ ] todo\n- [x] done\n';

/** Verbatim pampa JSON output for CONTENT (file name shortened). */
function taskListAstJson(): string {
    return JSON.stringify({
        blocks: [
            {
                c: [
                    [
                        {
                            c: [
                                { c: '☐', s: 2, t: 'Str' },
                                { s: 3, t: 'Space' },
                                { c: 'todo', s: 4, t: 'Str' },
                            ],
                            s: 1,
                            t: 'Plain',
                        },
                    ],
                    [
                        {
                            c: [
                                { c: '☒', s: 6, t: 'Str' },
                                { s: 7, t: 'Space' },
                                { c: 'done', s: 8, t: 'Str' },
                            ],
                            s: 5,
                            t: 'Plain',
                        },
                    ],
                ],
                s: 0,
                t: 'BulletList',
            },
        ],
        meta: {},
        'pandoc-api-version': [1, 23, 1],
        astContext: {
            files: [{ line_breaks: [10, 21], name: '/test.qmd', total_length: 22 }],
            p: [
                { d: 0, r: [0, 22], t: 0 },
                { d: 0, r: [6, 11], t: 0 },
                { d: 0, r: [2, 5], t: 0 },
                { d: 0, r: [5, 6], t: 0 },
                { d: 0, r: [6, 10], t: 0 },
                { d: 0, r: [17, 22], t: 0 },
                { d: 0, r: [13, 16], t: 0 },
                { d: 0, r: [16, 17], t: 0 },
                { d: 0, r: [17, 21], t: 0 },
            ],
        },
    });
}

function mountPreviewRoot(overrides: Partial<PreviewRootProps> = {}) {
    const setAst = vi.fn();
    const props: PreviewRootProps = {
        astJson: taskListAstJson(),
        untransformedAstJson: taskListAstJson(),
        renderedContent: CONTENT,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst,
        onNavigateToDocument: () => {},
        ...overrides,
    };
    return { setAst, ...render(<PreviewRoot {...props} />) };
}

describe('task-list rendering', () => {
    it('renders ul.task-list with label-wrapped checkboxes (writer parity)', () => {
        const { container } = mountPreviewRoot();
        const ul = container.querySelector('ul.task-list');
        expect(ul).not.toBeNull();
        const inputs = ul!.querySelectorAll('li > label > input[type="checkbox"]');
        expect(inputs.length).toBe(2);
        expect((inputs[0] as HTMLInputElement).checked).toBe(false);
        expect((inputs[1] as HTMLInputElement).checked).toBe(true);
        // The ballot-box characters must not leak into the visible text.
        expect(container.textContent).not.toContain('☐');
        expect(container.textContent).not.toContain('☒');
        expect(container.textContent).toContain('todo');
        expect(container.textContent).toContain('done');
    });

    it('toggling a checkbox commits a subtree edit with the flipped marker', () => {
        const { container, setAst } = mountPreviewRoot();
        const inputs = container.querySelectorAll('input[type="checkbox"]');
        fireEvent.click(inputs[0]);

        expect(setAst).toHaveBeenCalledTimes(1);
        const payload = setAst.mock.calls[0][0];
        expect(payload.__isPreviewNodeEdit).toBe(true);
        expect(payload.channel).toBe('subtree');
        // Destination: the BulletList's own source entry (whole-list range).
        const dest = JSON.parse(payload.destinationSourceInfoJson);
        expect(dest.t).toBe(0);
        expect(dest.r).toEqual([0, 22]);
        // The modified subtree flips item 0 to checked and keeps item 1.
        const doc = JSON.parse(payload.modifiedSubtreeJson);
        const list = doc.blocks[0];
        expect(list.t).toBe('BulletList');
        expect(list.c[0][0].c[0]).toMatchObject({ t: 'Str', c: '☒' });
        expect(list.c[1][0].c[0]).toMatchObject({ t: 'Str', c: '☒' });
    });

    it('unchecking a checked checkbox flips ☒ back to ☐', () => {
        const { container, setAst } = mountPreviewRoot();
        const inputs = container.querySelectorAll('input[type="checkbox"]');
        fireEvent.click(inputs[1]);

        const payload = setAst.mock.calls[0][0];
        const doc = JSON.parse(payload.modifiedSubtreeJson);
        const list = doc.blocks[0];
        expect(list.c[0][0].c[0]).toMatchObject({ t: 'Str', c: '☐' });
        expect(list.c[1][0].c[0]).toMatchObject({ t: 'Str', c: '☐' });
    });

    it('checkbox pointer events do not open the block editor', () => {
        // The block-edit surface activates on the host's React onPointerUp
        // (useBlockEditHover). A checkbox toggle must not double as an
        // editor-activation click (seen live: the toggle opened the item's
        // rich-text editor showing the raw ballot-box character).
        const { container } = mountPreviewRoot();
        const input = container.querySelector('input[type="checkbox"]')!;
        fireEvent.pointerDown(input, { pointerType: 'mouse' });
        fireEvent.pointerUp(input, { pointerType: 'mouse' });
        fireEvent.click(input);
        expect(container.querySelector('textarea')).toBeNull();
        expect(container.querySelector('[contenteditable="true"]')).toBeNull();
    });

    it('clicking the label text does not toggle the checkbox', () => {
        // Native <label> forwards text clicks to the wrapped input; in the
        // preview that click means "edit this item", not "flip its state".
        const { container, setAst } = mountPreviewRoot();
        const label = container.querySelector('ul.task-list li label')!;
        fireEvent.click(label);
        expect(setAst).not.toHaveBeenCalled();
    });

    it('renders disabled checkboxes and commits nothing when editing is disabled', () => {
        const { container, setAst } = mountPreviewRoot({ editingDisabled: true });
        const inputs = container.querySelectorAll('input[type="checkbox"]');
        expect(inputs.length).toBe(2);
        expect((inputs[0] as HTMLInputElement).disabled).toBe(true);
        fireEvent.click(inputs[0]);
        expect(setAst).not.toHaveBeenCalled();
    });
});

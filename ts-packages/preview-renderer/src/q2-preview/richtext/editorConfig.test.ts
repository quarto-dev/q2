// @vitest-environment jsdom
//
// bd-hafs0qho — `Mod-Enter` (the commit gesture) must NOT insert a hard break.
//
// tiptap's built-in HardBreak binds `Mod-Enter` → setHardBreak(). With a
// selection active (e.g. after select-all + bold), that REPLACES the selected
// text with a hard break; the editor then commits an empty block. This guards
// the fix: the shared config re-binds HardBreak to `Shift-Enter` only, so
// pressing `Mod-Enter` over a full selection leaves the text intact.

import { describe, it, expect, afterEach } from 'vitest';
import { Editor } from '@tiptap/core';
import { buildRichTextExtensions } from './editorConfig';

// Mirror prosemirror-keymap's `Mod` resolution: Meta on mac, Ctrl elsewhere.
const IS_MAC =
    typeof navigator !== 'undefined' && /Mac|iP(hone|[oa]d)/.test(navigator.platform);

function makeEditor(text: string): Editor {
    const el = document.createElement('div');
    document.body.appendChild(el);
    return new Editor({
        element: el,
        extensions: buildRichTextExtensions(),
        content: {
            type: 'doc',
            content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
        },
        autofocus: false,
        enableInputRules: false,
        enablePasteRules: false,
    });
}

function pressModEnter(editor: Editor): void {
    const ev = new KeyboardEvent('keydown', {
        key: 'Enter',
        code: 'Enter',
        bubbles: true,
        cancelable: true,
        metaKey: IS_MAC,
        ctrlKey: !IS_MAC,
    });
    editor.view.dom.dispatchEvent(ev);
}

function pressShiftEnter(editor: Editor): void {
    const ev = new KeyboardEvent('keydown', {
        key: 'Enter',
        code: 'Enter',
        bubbles: true,
        cancelable: true,
        shiftKey: true,
    });
    editor.view.dom.dispatchEvent(ev);
}

let editors: Editor[] = [];
function track(e: Editor): Editor {
    editors.push(e);
    return e;
}
afterEach(() => {
    editors.forEach(e => e.destroy());
    editors = [];
    document.body.innerHTML = '';
});

function hasHardBreak(editor: Editor): boolean {
    let found = false;
    editor.state.doc.descendants(node => {
        if (node.type.name === 'hardBreak') found = true;
    });
    return found;
}

describe('bd-hafs0qho — Mod-Enter must not insert a hard break', () => {
    it('Mod-Enter over a full selection leaves the text intact (no hard break)', () => {
        const editor = track(makeEditor('banana'));
        editor.commands.selectAll();

        pressModEnter(editor);

        expect(editor.state.doc.textContent).toBe('banana');
        expect(hasHardBreak(editor), 'Mod-Enter must not insert a hardBreak').toBe(false);
    });

    it('Mod-Enter at a collapsed caret does not append a trailing hard break', () => {
        const editor = track(makeEditor('banana'));
        editor.commands.focus('end');

        pressModEnter(editor);

        expect(editor.state.doc.textContent).toBe('banana');
        expect(hasHardBreak(editor)).toBe(false);
    });

    it('Shift-Enter still inserts a hard break (intentional line break preserved)', () => {
        const editor = track(makeEditor('banana'));
        editor.commands.focus('end');

        pressShiftEnter(editor);

        expect(hasHardBreak(editor), 'Shift-Enter must still insert a hardBreak').toBe(true);
        expect(editor.state.doc.textContent).toBe('banana');
    });
});

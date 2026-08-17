/**
 * caretFromClick against a REAL ProseMirror view in jsdom (bd-cpyq99ps).
 *
 * `caretFromClick.ts` documents its jsdom contract as "jsdom returns null from
 * posAtCoords" — i.e. every coordinate lookup is a MISS, so the caller falls
 * back to end-of-block focus. That claim was never tested: the unit tests
 * (`caretFromClick.test.ts`) drive a *fake* editor whose `posAtCoords` is a
 * `vi.fn()`, and `RichTextEditor.caret.integration.test.tsx` `vi.mock`s this
 * whole module. Nothing exercised the real ProseMirror code path.
 *
 * It was false. ProseMirror's `posAtCoords` calls
 * `(view.root.elementFromPoint ? view.root : doc).elementFromPoint(...)`, and
 * jsdom implements `elementFromPoint` on neither — so the fallback `doc` branch
 * threw `TypeError: ...elementFromPoint is not a function` rather than
 * returning null.
 *
 * That surfaced as a FLAKE, not a failure: the only caller is a
 * `requestAnimationFrame` in RichTextEditor's mount effect. When the frame
 * happened to fire while its test was still running, the throw was attributed
 * somewhere; when it fired after the test finished, it escaped as an unhandled
 * error and vitest failed the whole run with every test passing (48 files, 565
 * tests, "Errors 1 error").
 *
 * These tests pin the documented contract to the real implementation, so a
 * future jsdom/ProseMirror change that reintroduces the throw fails here —
 * deterministically, in one named test — instead of reappearing as an
 * intermittent red run somewhere else in the suite.
 */

import { describe, it, expect, afterEach } from 'vitest';
import { Editor } from '@tiptap/core';
import StarterKit from '@tiptap/starter-kit';
import { placeCaretFromClick, placeSelectionFromDrag } from './caretFromClick';

let editor: Editor | null = null;

afterEach(() => {
    editor?.destroy();
    editor = null;
});

/**
 * A genuine tiptap editor with a genuine ProseMirror `EditorView` — the point
 * of these tests is that `view.posAtCoords` is the real implementation, so
 * nothing here may be mocked.
 */
function realEditor(): Editor {
    const element = document.createElement('div');
    document.body.appendChild(element);
    return new Editor({
        element,
        extensions: [StarterKit],
        content: '<p>hello</p>',
        autofocus: false,
    });
}

describe('placeCaretFromClick against a real ProseMirror view in jsdom', () => {
    it('reports a miss instead of throwing', () => {
        editor = realEditor();

        // The regression itself: this threw TypeError before bd-cpyq99ps.
        expect(() => placeCaretFromClick(editor!, { x: 12, y: 34 })).not.toThrow();
    });

    it('returns false so the caller keeps its end-of-block default', () => {
        editor = realEditor();

        // jsdom has no layout, so every point is outside every box: a miss.
        // Returning `false` (rather than throwing, or claiming a bogus hit) is
        // what lets RichTextEditor's fallback chain reach `focus('end')`.
        expect(placeCaretFromClick(editor!, { x: 12, y: 34 })).toBe(false);
    });

    it('leaves the selection untouched on a miss', () => {
        editor = realEditor();
        const before = editor.state.selection.from;

        placeCaretFromClick(editor, { x: 12, y: 34 });

        expect(editor.state.selection.from).toBe(before);
    });
});

describe('placeSelectionFromDrag against a real ProseMirror view in jsdom', () => {
    it('reports a miss instead of throwing', () => {
        editor = realEditor();

        // Same posAtCoords path, called twice (drag anchor + head).
        expect(() =>
            placeSelectionFromDrag(editor!, { x: 5, y: 6 }, { x: 40, y: 6 }),
        ).not.toThrow();
    });

    it('returns false so the caller falls back to the caret path', () => {
        editor = realEditor();

        expect(
            placeSelectionFromDrag(editor, { x: 5, y: 6 }, { x: 40, y: 6 }),
        ).toBe(false);
    });
});

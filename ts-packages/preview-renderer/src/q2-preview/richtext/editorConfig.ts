// Shared tiptap extension config for the q2-preview rich-text editor.
//
// Extracted so the production editor (RichTextEditor.tsx) and the tests exercise
// the EXACT same extension set — no drift. The component adds one more extension
// on top (the commit keymap, which needs React callbacks); everything here is
// static and component-independent.

import StarterKit from '@tiptap/starter-kit';
import Subscript from '@tiptap/extension-subscript';
import Superscript from '@tiptap/extension-superscript';
import HardBreak from '@tiptap/extension-hard-break';
import type { AnyExtension } from '@tiptap/core';
import { Chip } from './chipExtension';

/**
 * The static extensions for the rich-text editor.
 *
 * Scope (1a/1b): paragraphs, headings, inline marks. Lists/quotes/code blocks
 * are disabled (structural editing is a later phase); `trailingNode: false`
 * avoids a phantom trailing paragraph.
 *
 * HardBreak (bd-hafs0qho): StarterKit's built-in HardBreak binds BOTH
 * `Mod-Enter` and `Shift-Enter` to `setHardBreak()`. `Mod-Enter` is our commit
 * gesture, and tiptap's keymap ran before the old DOM commit listener — so a
 * commit over a selection replaced the selected text with a hard break and wrote
 * an empty block. We disable the built-in HardBreak and re-add one bound to
 * `Shift-Enter` ONLY, so `Mod-Enter` never inserts a break. (The commit keymap
 * that binds `Mod-Enter` lives in RichTextEditor, since it needs React state.)
 */
export function buildRichTextExtensions(): AnyExtension[] {
    return [
        StarterKit.configure({
            heading: { levels: [1, 2, 3, 4, 5, 6] },
            blockquote: false,
            bulletList: false,
            orderedList: false,
            listItem: false,
            codeBlock: false,
            horizontalRule: false,
            trailingNode: false,
            link: { openOnClick: false },
            // Disable the built-in HardBreak (binds Mod-Enter + Shift-Enter); we
            // re-add a Shift-Enter-only variant below.
            hardBreak: false,
        }),
        HardBreak.extend({
            addKeyboardShortcuts() {
                // Shift-Enter inserts a hard break; Mod-Enter deliberately does
                // NOT (it is the commit gesture — see RichTextEditor).
                return {
                    'Shift-Enter': () => this.editor.commands.setHardBreak(),
                };
            },
        }),
        Subscript,
        Superscript,
        Chip,
    ];
}

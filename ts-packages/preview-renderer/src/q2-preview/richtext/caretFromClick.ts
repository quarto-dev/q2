// caretFromClick.ts (bd-q9lyghv2) — place the rich-text caret at the clicked spot.
//
// When a block is opened for rich-text editing by a MOUSE click, the original
// click event is already consumed by the time the tiptap editor mounts (the open
// goes through a React state update), so ProseMirror never gets to translate the
// click into a document position — the caret defaults to end-of-block, and only a
// SECOND click lands it correctly. We bridge that gap: the activation site stashes
// the click's viewport coordinates and the editor, at mount, replays them through
// `posAtCoords` to put the caret where the user actually clicked.
//
// This works because the editor renders the SAME visual text in the SAME measured
// box as the rendered block (same theme CSS), so the click's viewport coordinates
// land on the same glyph. Geometry correctness is browser-verified (jsdom returns
// null from posAtCoords); the unit tests here cover the hit/miss logic with a fake
// editor.

import type { Editor } from '@tiptap/core';
import { TextSelection } from '@tiptap/pm/state';

/**
 * Move the caret to the document position under the given viewport coordinates.
 *
 * @returns `true` if a position was resolved and the selection moved there;
 *   `false` if the point hit no content (caller keeps its end-of-block default).
 */
export function placeCaretFromClick(
    editor: Editor,
    coords: { x: number; y: number },
): boolean {
    // ProseMirror's posAtCoords takes {left, top} in viewport (client) space and
    // returns { pos, inside } or null when the point is outside any content.
    const hit = editor.view.posAtCoords({ left: coords.x, top: coords.y });
    if (!hit) return false;
    editor.chain().focus().setTextSelection(hit.pos).run();
    return true;
}

/**
 * Recreate a drag selection from the viewport coordinates of its two
 * endpoints (bd-abo9m23f). `anchor` is where the drag started, `head` where
 * it released — passed through in that order so a backward drag stays
 * backward and Shift-Arrow keeps extending from the release end.
 *
 * Uses `TextSelection.between`, which nudges each endpoint to the nearest
 * valid text position while preserving the anchor/head roles — the raw
 * `posAtCoords` hits may sit on structural boundaries.
 *
 * @returns `true` if both endpoints resolved to a non-empty selection and it
 *   was applied; `false` otherwise (either endpoint missed, or the endpoints
 *   collapse to a single position) — the caller falls back to
 *   `placeCaretFromClick` with `head`, i.e. the plain caret-at-release-point
 *   behavior.
 */
export function placeSelectionFromDrag(
    editor: Editor,
    anchor: { x: number; y: number },
    head: { x: number; y: number },
): boolean {
    const view = editor.view;
    const anchorHit = view.posAtCoords({ left: anchor.x, top: anchor.y });
    const headHit = view.posAtCoords({ left: head.x, top: head.y });
    if (!anchorHit || !headHit) return false;

    const { state } = view;
    const selection = TextSelection.between(
        state.doc.resolve(anchorHit.pos),
        state.doc.resolve(headHit.pos),
    );
    // Degenerate (both endpoints at one position, or no text positions found):
    // report a miss so the caller's caret fallback owns the collapsed case.
    if (selection.empty) return false;

    view.dispatch(state.tr.setSelection(selection));
    editor.commands.focus();
    return true;
}

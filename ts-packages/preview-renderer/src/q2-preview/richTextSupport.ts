/**
 * richTextSupport.ts — the single source of truth for "which blocks the
 * rich-text editor handles" and "is the rich editor actually showing".
 *
 * Kept in a tiny leaf module (no React, no component imports) so both the
 * dispatcher (which chooses the edit surface) and the breadcrumb (which chooses
 * between its inline and standalone renderings) can share the predicate without
 * an import cycle. bd-9x3zbuj8 Task 2.
 */

import type { PreviewContextValue } from './PreviewContext';

/**
 * Block types the rich-text editor can handle. Everything else falls back to the
 * textarea even when `richText` is on. 1a: Para. 1b: + Header.
 *
 * `Plain` (bd-7pxub583): a tight bullet/ordered list stores each item's content
 * as a `Plain` block (loose lists use `Para`); table cells are also `Plain`.
 * `Plain` seeds and round-trips exactly like `Para` — `astToProseMirror` maps
 * both to a paragraph node, and the text-commit channel's `preserve_leaf_variant`
 * (pampa `apply_node_edit.rs`) coerces the re-parsed `Paragraph` back to `Plain`
 * so editing a tight-list item does not silently loosen the list. Adding it here
 * makes tight-list-item and table-cell content rich-editable.
 */
export const RICHTEXT_SUPPORTED_TYPES = new Set<string>(['Para', 'Header', 'Plain']);

/** True when the rich editor is available for this block (flag on + supported type). */
export function richTextAvailable(ctx: PreviewContextValue, sourceNodeType: string): boolean {
    return !!ctx.richText && RICHTEXT_SUPPORTED_TYPES.has(sourceNodeType);
}

/**
 * True when the rich-text editor surface is actually showing for a block of this
 * node type — the rich editor is available AND the session mode is not 'plain'.
 * This is the exact condition `renderBlockEditSurface` uses to choose
 * `<RichTextEditor>` over the textarea. The breadcrumb uses it to decide between
 * the inline (in-toolbar) and standalone (floating) renderings.
 */
export function richEditorActiveForType(ctx: PreviewContextValue, sourceNodeType: string): boolean {
    return richTextAvailable(ctx, sourceNodeType) && (ctx.editorMode ?? 'rich') !== 'plain';
}

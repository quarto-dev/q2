import { createContext } from 'react';
import type { NoteInline } from '@quarto/preview-renderer/framework';

/**
 * Distributes JS-side note numbering to `Note.tsx`.
 *
 * The `WeakMap<NoteInline, number>` is keyed by object identity,
 * which the framework's walker-purity contract preserves: subtrees
 * with no `__quarto_custom_node` wrappers are returned by reference
 * across `unwrapCustomNodes`, so the `Note` reference PreviewRoot
 * captures pre-unwrap survives to the post-unwrap dispatch.
 *
 * This context is only meaningful for `reference-location: block`
 * and `section` configurations — the default `document` location
 * runs through `FootnotesTransform` upstream and replaces every
 * `Note` with `Span(Sup(Link))` before the AST reaches the iframe.
 *
 * Lifecycle: this whole subsystem is **temporary**. Once bd-1kly
 * extends `FootnotesTransform` to handle block/section uniformly,
 * raw `Note` inlines never reach the iframe under any config and
 * `Note.tsx` + `NoteNumberingContext` can be deleted.
 */
export const NoteNumberingContext = createContext<WeakMap<NoteInline, number>>(
    new WeakMap(),
);

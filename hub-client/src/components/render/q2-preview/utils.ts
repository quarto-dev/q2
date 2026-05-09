/**
 * Shared utilities for q2-preview's built-in components.
 *
 * 2B subset:
 *  - `lookupAssetUrl`: external-URL passthrough + manifest lookup with
 *    fallback to the original URL on miss (broken-image affordance).
 *  - `inlinesToPlainText`: Pandoc-Stringify-equivalent walk for alt
 *    text, tooltip bodies, and other plain-text contexts.
 *  - `blocksToPlainText`: companion walk used by Note.tsx's `title=`
 *    attribute (250-char cap).
 *
 * Plan 2C extends with `formatRefLabel`, `composeAttr`, `renderSlot`
 * for the CustomNode components.
 */

import type { BlockNode, InlineNode } from '../framework';

// --- asset URL lookup ------------------------------------------------

/**
 * Resolve an Image `target.0` URL string to the URL the iframe should
 * actually load. External URLs (`https?:`, `data:`, `//`) pass through
 * unchanged; project-relative paths look up in the manifest. On
 * manifest miss, the original URL is returned — the resulting broken
 * `<img>` is a deliberate signal that resolution failed (silently
 * swallowing missing images would hide bugs in the walker).
 */
export function lookupAssetUrl(
    manifest: Record<string, string>,
    url: string,
): string {
    if (
        url.startsWith('https://') ||
        url.startsWith('http://') ||
        url.startsWith('data:') ||
        url.startsWith('//')
    ) {
        return url;
    }
    return manifest[url] ?? url;
}

// --- plain-text walks ------------------------------------------------

/**
 * Pandoc Stringify-equivalent for a list of inlines. Handles every
 * inline that v1 q2-preview can plausibly carry; unknown variants
 * contribute the empty string (rather than throwing) so a future
 * inline addition doesn't break alt text emission.
 */
export function inlinesToPlainText(inlines: InlineNode[]): string {
    let out = '';
    for (const inl of inlines) out += inlineText(inl);
    return out;
}

function inlineText(node: InlineNode): string {
    const n = node as { t: string; c?: unknown };
    switch (n.t) {
        case 'Str':
            return (n.c as string) ?? '';
        case 'Space':
        case 'SoftBreak':
            return ' ';
        case 'LineBreak':
            return '\n';
        case 'Emph':
        case 'Strong':
        case 'Underline':
        case 'Strikeout':
        case 'Superscript':
        case 'Subscript':
        case 'SmallCaps':
            return inlinesToPlainText((n.c as InlineNode[]) ?? []);
        case 'Code':
            // Code: c = [attr, content]
            return (n.c as [unknown, string])[1] ?? '';
        case 'Link':
        case 'Image': {
            // c = [attr, inlines, [url, title]]
            const c = n.c as [unknown, InlineNode[], [string, string]];
            return inlinesToPlainText(c[1] ?? []);
        }
        case 'Span':
        case 'Quoted':
            // c = [meta, inlines]
            return inlinesToPlainText(((n.c as [unknown, InlineNode[]]) ?? [null, []])[1] ?? []);
        case 'Math':
            // c = [{t: '…Math'}, latex]
            return ((n.c as [unknown, string]) ?? [null, ''])[1] ?? '';
        case 'RawInline':
            // c = [format, content]; v1 includes the raw content for plain-text contexts.
            return ((n.c as [string, string]) ?? ['', ''])[1] ?? '';
        case 'Cite': {
            // c = [citations, inlines]; render the visible inlines.
            const c = n.c as [unknown[], InlineNode[]];
            return inlinesToPlainText(c?.[1] ?? []);
        }
        case 'Note':
            // For plain-text contexts, the note body itself is the text.
            return blocksToPlainText((n.c as BlockNode[]) ?? []);
        case 'CustomInline':
            // Fall through: walk slot inlines if available.
            return '';
        default:
            return '';
    }
}

/**
 * Companion to `inlinesToPlainText` — walks blocks into plain text.
 * Used by `Note.tsx`'s `title=` tooltip attribute. Block boundaries
 * join with a single space (the consumer is a hover tooltip, not a
 * typeset document — paragraph breaks aren't meaningful).
 */
export function blocksToPlainText(blocks: BlockNode[]): string {
    const parts: string[] = [];
    for (const b of blocks) parts.push(blockText(b));
    return parts.filter((p) => p.length > 0).join(' ');
}

function blockText(node: BlockNode): string {
    const n = node as { t: string; c?: unknown };
    switch (n.t) {
        case 'Para':
        case 'Plain':
            return inlinesToPlainText((n.c as InlineNode[]) ?? []);
        case 'Header': {
            // c = [level, attr, inlines]
            const c = n.c as [number, unknown, InlineNode[]];
            return inlinesToPlainText(c?.[2] ?? []);
        }
        case 'CodeBlock':
        case 'RawBlock': {
            // c = [attr|format, content]
            const c = n.c as [unknown, string];
            return c?.[1] ?? '';
        }
        case 'BlockQuote':
            return blocksToPlainText((n.c as BlockNode[]) ?? []);
        case 'Div': {
            // c = [attr, blocks]
            const c = n.c as [unknown, BlockNode[]];
            return blocksToPlainText(c?.[1] ?? []);
        }
        case 'Figure': {
            // c = [attr, [shortCaption, captionBlocks], bodyBlocks]
            const c = n.c as [unknown, [InlineNode[] | null, BlockNode[]], BlockNode[]];
            const caption = blocksToPlainText(c?.[1]?.[1] ?? []);
            const body = blocksToPlainText(c?.[2] ?? []);
            return [caption, body].filter((s) => s.length > 0).join(' ');
        }
        case 'BulletList': {
            // c = BlockNode[][]
            const items = (n.c as BlockNode[][]) ?? [];
            return items.map((it) => blocksToPlainText(it)).join(' ');
        }
        case 'OrderedList': {
            // c = [[start, style, delim], BlockNode[][]]
            const c = n.c as [unknown, BlockNode[][]];
            return (c?.[1] ?? []).map((it) => blocksToPlainText(it)).join(' ');
        }
        case 'LineBlock': {
            const lines = (n.c as InlineNode[][]) ?? [];
            return lines.map(inlinesToPlainText).join(' ');
        }
        case 'DefinitionList': {
            // c = [InlineNode[], BlockNode[][]][]
            const items = (n.c as [InlineNode[], BlockNode[][]][]) ?? [];
            const out: string[] = [];
            for (const [term, defs] of items) {
                out.push(inlinesToPlainText(term));
                for (const d of defs) out.push(blocksToPlainText(d));
            }
            return out.join(' ');
        }
        case 'HorizontalRule':
        case 'CustomBlock':
            return '';
        default:
            return '';
    }
}

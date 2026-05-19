/**
 * Pandoc-AST plain-text walks (framework-tier).
 *
 * Pure shape concerns: no format-specific behavior. Lifted from
 * `q2-preview/utils.tsx` (where they originally landed in 2pre but
 * had no q2-preview-specific behavior) so framework-tier consumers
 * — `framework/meta.ts`'s `extractMetaString` for MetaInlines /
 * MetaBlocks coercion, the slide renderer's meta walk, and any
 * future format renderer — can use them without crossing format
 * boundaries.
 *
 * Pandoc-Stringify equivalent: walks every inline / block variant
 * v1 can plausibly carry; unknown variants contribute the empty
 * string (rather than throwing) so a future inline addition doesn't
 * break alt-text emission or meta-string coercion.
 */

import type { BlockNode, InlineNode } from './types';

/**
 * Pandoc Stringify-equivalent for a list of inlines.
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
 * Block boundaries join with a single space (the consumers are hover
 * tooltips and meta-string coercion, not typeset documents —
 * paragraph breaks aren't meaningful).
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

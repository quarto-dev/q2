// Phase 1 (bd-sjb4pzx8) — Pandoc untransformed AST -> ProseMirror document.
//
// The "seed" direction: build the editor's document from the typed AST we already
// have (resolved.sourceNode + the source-info pool), NOT by re-lexing markdown.
// Opaque constructs (Math, Cite, Span.quarto-shortcode__, RawInline, and — as a
// whole-block fallback — Div/RawBlock/Table/Figure/DefinitionList) become verbatim
// `chip` atoms by slicing source at their byte range.
//
// Returns a ProseMirror Node built against `richTextSchema`. The live editor is
// seeded with `.toJSON()` of this node (tiptap re-parses it against its
// name-identical schema); tests serialize the node directly.

import type { Mark, Node as PMNode } from '@tiptap/pm/model';
import { richTextSchema, type ChipKind } from './schema';
import { type AstNode, type PoolEntry, nodeSource } from './ast';

interface Ctx {
  pool: PoolEntry[];
  src: string;
  /** opaque node types we could not richly represent (reported for diagnostics) */
  unknown: Set<string>;
}

const S = richTextSchema;
const M = S.marks;
const N = S.nodes;

function chip(node: AstNode, kind: ChipKind, ctx: Ctx, fallback: string): PMNode {
  const src = nodeSource(node, ctx.pool, ctx.src) ?? fallback;
  return N.chip.create({ src, kind });
}

function asArray(c: unknown): AstNode[] {
  return Array.isArray(c) ? (c as AstNode[]) : [];
}

// ---- inlines ---------------------------------------------------------------

function inlines(items: AstNode[], marks: readonly Mark[], ctx: Ctx): PMNode[] {
  const out: PMNode[] = [];
  for (const node of items) {
    switch (node.t) {
      case 'Str':
        out.push(S.text(node.c as string, marks));
        break;
      case 'Space':
      case 'SoftBreak':
        // SoftBreak collapses to a space (reformatted-but-equivalent).
        out.push(S.text(' ', marks));
        break;
      case 'LineBreak':
        out.push(N.hardBreak.create(undefined, undefined, marks));
        break;
      case 'Emph':
        out.push(...inlines(asArray(node.c), marks.concat(M.italic.create()), ctx));
        break;
      case 'Strong':
        out.push(...inlines(asArray(node.c), marks.concat(M.bold.create()), ctx));
        break;
      case 'Strikeout':
        out.push(...inlines(asArray(node.c), marks.concat(M.strike.create()), ctx));
        break;
      case 'Subscript':
        out.push(...inlines(asArray(node.c), marks.concat(M.subscript.create()), ctx));
        break;
      case 'Superscript':
        out.push(...inlines(asArray(node.c), marks.concat(M.superscript.create()), ctx));
        break;
      case 'Quoted': {
        // Pandoc Quoted is [QuoteType, Inline[]]. Unlike Emph/Strong (rendered
        // as marks) a quote's visible form IS its delimiter characters, so we
        // emit literal STRAIGHT quotes around still-WYSIWYG content (marks and
        // nested quotes recurse). Straight quotes round-trip: pampa's reader
        // re-parses "…"/'…' back into Quoted and its qmd writer emits straight
        // quotes; prosemirror-markdown's esc() never escapes `"`/`'`. (bd-iwv3708i)
        const [qt, content] = (node.c as [{ t?: string }, AstNode[]]) ?? [{}, []];
        const q = qt?.t === 'SingleQuote' ? "'" : '"';
        out.push(S.text(q, marks));
        out.push(...inlines(asArray(content), marks, ctx));
        out.push(S.text(q, marks));
        break;
      }
      case 'Underline':
      case 'SmallCaps':
        // Outside the v1 mark set -> chip the whole construct verbatim.
        out.push(chip(node, 'span', ctx, ''));
        break;
      case 'Code': {
        const [, code] = node.c as [unknown, string];
        out.push(S.text(code, marks.concat(M.code.create())));
        break;
      }
      case 'Link': {
        const [, label, target] = node.c as [unknown, AstNode[], [string, string]];
        const link = M.link.create({ href: target[0], title: target[1] || null });
        out.push(...inlines(label, marks.concat(link), ctx));
        break;
      }
      case 'Math':
        out.push(chip(node, 'math', ctx, ''));
        break;
      case 'Cite':
        out.push(chip(node, 'cite', ctx, ''));
        break;
      case 'RawInline':
        out.push(chip(node, 'raw', ctx, (node.c as [string, string])?.[1] ?? ''));
        break;
      case 'Span': {
        const attr = (node.c as [unknown])?.[0] as [string, string[], [string, string][]];
        const classes = attr?.[1] ?? [];
        const isShortcode = classes.includes('quarto-shortcode__');
        out.push(chip(node, isShortcode ? 'shortcode' : 'span', ctx, ''));
        break;
      }
      case 'Image':
      case 'Note':
        out.push(chip(node, 'raw', ctx, ''));
        break;
      default:
        ctx.unknown.add(`inline:${node.t}`);
        out.push(chip(node, 'raw', ctx, ''));
        break;
    }
  }
  return out;
}

// ---- blocks ----------------------------------------------------------------

function listItems(itemsC: unknown, ctx: Ctx): PMNode[] {
  const items = (itemsC as AstNode[][]) ?? [];
  return items.map((itemBlocks) => N.listItem.create(null, blocks(itemBlocks, ctx)));
}

/** A Pandoc list is tight when its items hold `Plain` (not `Para`) blocks. */
function isTight(itemsC: unknown): boolean {
  const items = (itemsC as AstNode[][]) ?? [];
  return items.every((item) => !item.some((b) => b.t === 'Para'));
}

function blocks(items: AstNode[], ctx: Ctx): PMNode[] {
  const out: PMNode[] = [];
  for (const node of items) {
    switch (node.t) {
      case 'Para':
      case 'Plain':
        out.push(N.paragraph.create(null, inlines(asArray(node.c), [], ctx)));
        break;
      case 'Header': {
        const [level, , inl] = node.c as [number, unknown, AstNode[]];
        out.push(N.heading.create({ level }, inlines(inl, [], ctx)));
        break;
      }
      case 'BulletList':
        out.push(N.bulletList.create({ tight: isTight(node.c) }, listItems(node.c, ctx)));
        break;
      case 'OrderedList': {
        const [attrs, its] = node.c as [[number, unknown, unknown], unknown];
        out.push(N.orderedList.create({ start: attrs?.[0] ?? 1, tight: isTight(its) }, listItems(its, ctx)));
        break;
      }
      case 'BlockQuote':
        out.push(N.blockquote.create(null, blocks(asArray(node.c), ctx)));
        break;
      case 'CodeBlock': {
        const [attr, code] = node.c as [[string, string[], [string, string][]], string];
        const classes = attr?.[1] ?? [];
        // pampa stores the fence info string as a class verbatim, INCLUDING braces
        // for executable cells (```{python} -> "{python}"; ```python -> "python").
        const language = classes.join(' ');
        out.push(N.codeBlock.create({ language }, code ? [S.text(code)] : []));
        break;
      }
      case 'RawBlock':
      case 'Div':
      case 'Table':
      case 'Figure':
      case 'DefinitionList':
        // v1: whole opaque block -> paragraph wrapping a verbatim chip. (Real
        // integration "reaches into" a Div and edits inner blocks individually;
        // this whole-block chip is only the bridge's safe fallback.)
        out.push(N.paragraph.create(null, [chip(node, 'block', ctx, '')]));
        break;
      case 'HorizontalRule':
      case 'Null':
        break;
      default:
        ctx.unknown.add(`block:${node.t}`);
        out.push(N.paragraph.create(null, [chip(node, 'block', ctx, '')]));
        break;
    }
  }
  return out;
}

export interface AstToDocResult {
  doc: PMNode;
  unknown: string[];
}

/** Build a ProseMirror doc from one or more untransformed Pandoc blocks. */
export function astToDoc(sourceBlocks: AstNode[], pool: PoolEntry[], src: string): AstToDocResult {
  const ctx: Ctx = { pool, src, unknown: new Set() };
  const content = blocks(sourceBlocks, ctx);
  // A doc must contain at least one block; fall back to an empty paragraph.
  const doc = N.doc.create(null, content.length ? content : [N.paragraph.create()]);
  return { doc, unknown: [...ctx.unknown] };
}

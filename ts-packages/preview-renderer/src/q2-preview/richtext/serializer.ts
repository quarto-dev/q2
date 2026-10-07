// Phase 1 (bd-sjb4pzx8) — ProseMirror document -> markdown text.
//
// Reuses prosemirror-markdown's validated default rules, re-keyed to tiptap's
// node/mark NAMES, with custom rules where tiptap's attrs differ from
// prosemirror-markdown's (codeBlock.language vs code_block.params; orderedList.start
// vs ordered_list.order) and for the `chip` atom (verbatim, unescaped). The same
// serializer runs over both the live tiptap editor's doc and the test schema's doc.

import {
  MarkdownSerializer,
  MarkdownSerializerState,
  defaultMarkdownSerializer,
} from 'prosemirror-markdown';
import { EDITORIAL_SIGILS, type EditorialKind } from './schema';
import { Fragment, type Mark as PMMark, type Node as PMNode } from '@tiptap/pm/model';

const d = defaultMarkdownSerializer;

type NodeRule = (state: MarkdownSerializerState, node: PMNode, parent: PMNode, index: number) => void;

/**
 * Hidden `[>> …]` comments (span comments prototype): a block's own comments
 * ride on `node.attrs.comments` as verbatim source and are written back at the
 * END of the block (their original position among the inlines is not kept).
 */
function writeHiddenComments(state: MarkdownSerializerState, node: PMNode) {
  const comments = (node.attrs.comments as string[] | undefined) ?? [];
  for (const c of comments) state.write(' ' + c);
}

/** `{#id .class key="value"}` for a Pandoc attr tuple; empty string for an empty attr. */
function attrString(attr: unknown): string {
  const [id, classes, kvs] = (attr as [string, string[], [string, string][]]) ?? ['', [], []];
  const parts: string[] = [];
  if (id) parts.push('#' + id);
  for (const c of classes ?? []) parts.push('.' + c);
  for (const [k, v] of kvs ?? []) parts.push(`${k}="${String(v).replace(/"/g, '\\"')}"`);
  return parts.length ? `{${parts.join(' ')}}` : '';
}

const nodes: Record<string, NodeRule> = {
  paragraph(state, node) {
    state.renderInline(node);
    writeHiddenComments(state, node);
    state.closeBlock(node);
  },
  heading(state, node) {
    state.write(state.repeat('#', node.attrs.level as number) + ' ');
    state.renderInline(node);
    writeHiddenComments(state, node);
    state.closeBlock(node);
  },
  blockquote: d.nodes.blockquote as NodeRule,
  bulletList: d.nodes.bullet_list as NodeRule,
  listItem: d.nodes.list_item as NodeRule,
  hardBreak: d.nodes.hard_break as NodeRule,
  text: d.nodes.text as NodeRule,

  // tiptap codeBlock carries `language`; prosemirror-markdown's code_block reads
  // `params`. Emit a fence + the language verbatim (handles ```{python} and ```python).
  codeBlock(state, node) {
    const language = (node.attrs.language as string) || '';
    state.write('```' + language + '\n');
    state.text(node.textContent, false);
    state.ensureNewLine();
    state.write('```');
    state.closeBlock(node);
  },

  // tiptap orderedList carries `start`; prosemirror-markdown's ordered_list reads
  // `order`. Replicate the default rule against `start`.
  orderedList(state, node) {
    const start = (node.attrs.start as number) || 1;
    const maxW = String(start + node.childCount - 1).length;
    const space = state.repeat(' ', maxW + 2);
    state.renderList(node, space, (i) => {
      const nStr = String(start + i);
      return state.repeat(' ', maxW - nStr.length) + nStr + '. ';
    });
  },

  // Opaque construct: emit the verbatim source, UNESCAPED (so `{`, `$`, `@`, `<`
  // survive). The chip round-trips byte-for-byte.
  chip(state, node) {
    state.text(node.attrs.src as string, false);
  },
};

const marks = {
  bold: d.marks.strong,
  // qmd disallows `***` (triple-star). Use `_` for italic so bold+italic
  // serializes as `**_…_**` (valid) instead of `***…***` (rejected by pampa).
  // `_` is intraword-safe, so this is the better qmd choice regardless.
  italic: { open: '_', close: '_', mixable: true, expelEnclosingWhitespace: true },
  code: d.marks.code,
  link: d.marks.link,
  strike: { open: '~~', close: '~~', mixable: true, expelEnclosingWhitespace: true },
  // qmd subscript `~x~` / superscript `^x^` (Pandoc). Single tilde for subscript
  // (double `~~` is strikethrough, above).
  subscript: { open: '~', close: '~', mixable: true, expelEnclosingWhitespace: true },
  superscript: { open: '^', close: '^', mixable: true, expelEnclosingWhitespace: true },
  // Authored span: `[text]{attrs}`; the comments hidden inside it come back
  // just before the closing bracket — `[text [>> c]]{attrs}`. NOT mixable and
  // the lowest-ranked mark (schema order), so it is always the outermost
  // delimiter pair: other marks split around a span edge, never vice versa.
  span: {
    open: (_state: MarkdownSerializerState, mark: PMMark) => {
      const kind = mark.attrs.kind as EditorialKind | '';
      return kind ? `[${EDITORIAL_SIGILS[kind]} ` : '[';
    },
    close: (_state: MarkdownSerializerState, mark: PMMark) => {
      const comments = (mark.attrs.comments as string[] | undefined) ?? [];
      return comments.map((c) => ' ' + c).join('') + ']' + attrString(mark.attrs.attr);
    },
    mixable: false,
    expelEnclosingWhitespace: true,
  },
};

export const richTextSerializer = new MarkdownSerializer(nodes, marks);

/**
 * The marks that stay open across the boundary between two adjacent text
 * nodes: the common PREFIX of their (rank-ordered) mark lists. A mark deeper
 * than the first difference has to close and reopen even if both sides carry
 * it (bold continuing into a span: `[bold]` vs `[span, bold]` -> prefix `[]`).
 */
function marksKeptAcross(a: PMNode, b: PMNode): readonly PMMark[] {
  let n = 0;
  while (n < a.marks.length && n < b.marks.length && a.marks[n].eq(b.marks[n])) n++;
  return a.marks.slice(0, n);
}

/**
 * Whitespace at a mark boundary may only carry the marks that stay open
 * across it. prosemirror-markdown's `expelEnclosingWhitespace` only handles a
 * mark that ENDS at whitespace; when a mark merely closes and reopens around
 * a new outer mark (bold continuing into a span: `**bold words** plain` with a
 * span from `words`), the space stays inside and serializes as
 * `**bold **[**words** …]` — invalid emphasis. Rebuild each text block so the
 * trailing/leading whitespace on either side of a marks change is its own
 * text node carrying just the marks kept across: `**bold** [**words** plain]`.
 * Code marks are left alone (their whitespace is content).
 */
function unmarkBoundaryWhitespace(doc: PMNode): PMNode {
  const code = doc.type.schema.marks.code;
  const isCode = (n: PMNode) => !!code && code.isInSet(n.marks) !== undefined;
  const rebuild = (node: PMNode): PMNode => {
    if (!node.isTextblock) {
      if (node.isLeaf) return node;
      return node.copy(Fragment.fromArray(node.content.content.map(rebuild)));
    }
    const schema = node.type.schema;
    const kids = node.content.content;
    const out: PMNode[] = [];
    for (let i = 0; i < kids.length; i++) {
      const cur = kids[i];
      if (!cur.isText || !cur.text || cur.marks.length === 0 || isCode(cur)) {
        out.push(cur);
        continue;
      }
      const prev = kids[i - 1];
      const next = kids[i + 1];
      let text = cur.text;
      let lead: PMNode | null = null;
      let trail: PMNode | null = null;
      if (prev?.isText) {
        const kept = marksKeptAcross(prev, cur);
        const m = /^\s+/.exec(text);
        if (kept.length < cur.marks.length && m && m[0].length < text.length) {
          lead = schema.text(m[0], kept);
          text = text.slice(m[0].length);
        }
      }
      if (next?.isText) {
        const kept = marksKeptAcross(cur, next);
        const m = /\s+$/.exec(text);
        if (kept.length < cur.marks.length && m && m[0].length < text.length) {
          trail = schema.text(m[0], kept);
          text = text.slice(0, text.length - m[0].length);
        }
      }
      if (lead) out.push(lead);
      out.push(schema.text(text, cur.marks));
      if (trail) out.push(trail);
    }
    return node.copy(Fragment.fromArray(out));
  };
  return rebuild(doc);
}

/** Serialize a ProseMirror document to markdown text. */
export function docToMarkdown(doc: PMNode): string {
  return richTextSerializer.serialize(unmarkBoundaryWhitespace(doc));
}

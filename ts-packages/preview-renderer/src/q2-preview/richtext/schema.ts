// Phase 1 (bd-sjb4pzx8) — ProseMirror schema for the rich-text block editor.
//
// Node/mark NAMES match tiptap StarterKit v3 (paragraph, heading, bulletList,
// orderedList, listItem, blockquote, codeBlock, hardBreak, text; marks bold,
// italic, code, strike, link) plus a custom `chip` inline atom for opaque
// Quarto constructs (shortcodes, math, @crossref, [@cite], raw inline). Because
// the names match, one `astToDoc` builder + one markdown serializer serve BOTH
// this standalone schema (used in round-trip tests) and the live tiptap editor
// (which is fed `astToDoc(...).toJSON()` and re-parses it against its own,
// name-identical schema).
//
// We import prosemirror-model from `@tiptap/pm/model` so the Node instances this
// schema produces are the exact same class the tiptap editor uses.

import { Schema, type NodeSpec, type MarkSpec, type DOMOutputSpec } from '@tiptap/pm/model';

export type ChipKind = 'math' | 'cite' | 'shortcode' | 'span' | 'raw' | 'block';

const pDOM: DOMOutputSpec = ['p', 0];
const blockquoteDOM: DOMOutputSpec = ['blockquote', 0];
const ulDOM: DOMOutputSpec = ['ul', 0];
const olDOM: DOMOutputSpec = ['ol', 0];
const liDOM: DOMOutputSpec = ['li', 0];
const brDOM: DOMOutputSpec = ['br'];
const codeDOM: DOMOutputSpec = ['pre', ['code', 0]];

const nodes: Record<string, NodeSpec> = {
  doc: { content: 'block+' },

  paragraph: {
    group: 'block',
    content: 'inline*',
    // Verbatim source of the block's own `[>> …]` comments, hidden from the
    // editable text and re-appended on serialization (span comments prototype;
    // see spanMarkExtension.ts for the tiptap twin).
    attrs: { comments: { default: [] } },
    parseDOM: [{ tag: 'p' }],
    toDOM: () => pDOM,
  },

  heading: {
    group: 'block',
    content: 'inline*',
    attrs: { level: { default: 1 }, comments: { default: [] } },
    defining: true,
    parseDOM: [1, 2, 3, 4, 5, 6].map((level) => ({ tag: `h${level}`, attrs: { level } })),
    toDOM: (node) => [`h${node.attrs.level as number}`, 0],
  },

  blockquote: {
    group: 'block',
    content: 'block+',
    defining: true,
    parseDOM: [{ tag: 'blockquote' }],
    toDOM: () => blockquoteDOM,
  },

  codeBlock: {
    group: 'block',
    content: 'text*',
    marks: '',
    code: true,
    defining: true,
    attrs: { language: { default: '' } },
    parseDOM: [{ tag: 'pre', preserveWhitespace: 'full' }],
    toDOM: () => codeDOM,
  },

  bulletList: {
    group: 'block',
    content: 'listItem+',
    // `tight` drives prosemirror-markdown's renderList: tight (Pandoc Plain items)
    // emits single-newline items; loose (Para items) emits blank-line-separated.
    attrs: { tight: { default: true } },
    parseDOM: [{ tag: 'ul' }],
    toDOM: () => ulDOM,
  },

  orderedList: {
    group: 'block',
    content: 'listItem+',
    attrs: { start: { default: 1 }, tight: { default: true } },
    parseDOM: [{ tag: 'ol' }],
    toDOM: () => olDOM,
  },

  listItem: {
    content: 'block+',
    defining: true,
    parseDOM: [{ tag: 'li' }],
    toDOM: () => liDOM,
  },

  text: { group: 'inline' },

  hardBreak: {
    group: 'inline',
    inline: true,
    selectable: false,
    parseDOM: [{ tag: 'br' }],
    toDOM: () => brDOM,
  },

  chip: {
    group: 'inline',
    inline: true,
    atom: true,
    selectable: true,
    attrs: { src: {}, kind: { default: 'raw' } },
    parseDOM: [
      {
        tag: 'span.q2-chip',
        getAttrs(dom) {
          const el = dom as HTMLElement;
          return {
            src: el.getAttribute('data-src') ?? '',
            kind: (el.className.match(/q2-chip-(\w+)/)?.[1] ?? 'raw') as ChipKind,
          };
        },
      },
    ],
    toDOM(node) {
      const { src, kind } = node.attrs as { src: string; kind: ChipKind };
      return ['span', { class: `q2-chip q2-chip-${kind}`, 'data-src': src, contenteditable: 'false' }, src];
    },
  },
};

const marks: Record<string, MarkSpec> = {
  // `span` is FIRST on purpose: schema order is mark rank, and the lowest rank
  // is the outermost mark when serializing (see the tiptap twin's `priority`).
  // An authored `[text]{attrs}` span as an editable mark (span comments
  // prototype). `attr` is the Pandoc attr tuple; `comments` the verbatim source
  // of `[>> …]` comments that were inside the span (hidden, written back).
  span: {
    attrs: { attr: { default: ['', [], []] }, comments: { default: [] } },
    inclusive: false,
    parseDOM: [
      {
        tag: 'span.q2-rt-span',
        getAttrs(dom) {
          const el = dom as HTMLElement;
          const parse = (raw: string | null, fallback: unknown) => {
            try {
              return raw ? JSON.parse(raw) : fallback;
            } catch {
              return fallback;
            }
          };
          return {
            attr: parse(el.getAttribute('data-attr'), ['', [], []]),
            comments: parse(el.getAttribute('data-comments'), []),
          };
        },
      },
    ],
    toDOM(mark) {
      return [
        'span',
        {
          class: 'q2-rt-span',
          'data-attr': JSON.stringify(mark.attrs.attr),
          'data-comments': JSON.stringify(mark.attrs.comments),
        },
        0,
      ];
    },
  },
  bold: { parseDOM: [{ tag: 'strong' }, { tag: 'b' }], toDOM: () => ['strong', 0] },
  italic: { parseDOM: [{ tag: 'em' }, { tag: 'i' }], toDOM: () => ['em', 0] },
  strike: { parseDOM: [{ tag: 's' }, { tag: 'del' }], toDOM: () => ['s', 0] },
  subscript: { parseDOM: [{ tag: 'sub' }], toDOM: () => ['sub', 0] },
  superscript: { parseDOM: [{ tag: 'sup' }], toDOM: () => ['sup', 0] },
  code: { parseDOM: [{ tag: 'code' }], toDOM: () => ['code', 0] },
  link: {
    attrs: { href: {}, title: { default: null } },
    inclusive: false,
    parseDOM: [
      {
        tag: 'a[href]',
        getAttrs(dom) {
          const el = dom as HTMLElement;
          return { href: el.getAttribute('href'), title: el.getAttribute('title') };
        },
      },
    ],
    toDOM(mark) {
      const { href, title } = mark.attrs as { href: string; title: string | null };
      return ['a', { href, title }, 0];
    },
  },
};

export const richTextSchema = new Schema({ nodes, marks });

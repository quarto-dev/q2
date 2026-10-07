// Span comments prototype — tiptap extensions for authored spans and hidden
// editorial comments in the rich-text editor.
//
//  - `SpanMark` (`span`): an authored `[text]{attrs}` span as an EDITABLE mark,
//    like bold/italic, rather than an opaque chip. Its attrs carry the span's
//    Pandoc attr tuple and the verbatim source of any `[>> …]` comments that
//    were inside it — the comments are not shown, but ride along and are
//    written back by the serializer (`[text [>> c]]{attrs}`).
//  - `HiddenComments`: a `comments` attribute on paragraph/heading holding the
//    verbatim source of the block's own comments (stripped from the editable
//    text; re-appended at the end of the block on serialization).
//
// Names + attrs match `richtext/schema.ts` so the seed doc (built by `astToDoc`
// and handed to tiptap as JSON) parses cleanly here.

import { Extension, Mark, mergeAttributes } from '@tiptap/core';

/** Pandoc attr tuple: [id, classes, key-values]. */
export type SpanAttr = [string, string[], [string, string][]];

export const EMPTY_SPAN_ATTR: SpanAttr = ['', [], []];

const parseJson = <T,>(raw: string | null, fallback: T): T => {
  if (!raw) return fallback;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
};

export const SpanMark = Mark.create({
  name: 'span',
  // Highest priority so the span is the FIRST mark in the schema, hence the
  // OUTERMOST mark on any text: the serializer then splits bold/italic around
  // a span boundary (`**a** [**b** c]`) rather than the span around theirs.
  // (StarterKit's Link is 1000; this must beat it.)
  priority: 2000,
  // Like link: typing at the edge does not extend the span.
  inclusive: false,

  addAttributes() {
    return {
      attr: { default: EMPTY_SPAN_ATTR, rendered: false },
      comments: { default: [] as string[], rendered: false },
      kind: { default: '', rendered: false },
    };
  },

  parseHTML() {
    return [
      {
        tag: 'span.q2-rt-span',
        getAttrs(dom) {
          const el = dom as HTMLElement;
          return {
            attr: parseJson<SpanAttr>(el.getAttribute('data-attr'), EMPTY_SPAN_ATTR),
            comments: parseJson<string[]>(el.getAttribute('data-comments'), []),
            kind: el.getAttribute('data-kind') ?? '',
          };
        },
      },
    ];
  },

  renderHTML({ HTMLAttributes, mark }) {
    const comments = (mark.attrs.comments as string[]) ?? [];
    return [
      'span',
      mergeAttributes(HTMLAttributes, {
        class: `q2-rt-span${mark.attrs.kind ? ` q2-rt-span-${mark.attrs.kind}` : ''}${comments.length ? ' q2-rt-span-commented' : ''}`,
        'data-kind': mark.attrs.kind,
        'data-attr': JSON.stringify(mark.attrs.attr ?? EMPTY_SPAN_ATTR),
        'data-comments': JSON.stringify(comments),
        title: comments.length
          ? `span · ${comments.length} hidden comment${comments.length === 1 ? '' : 's'}`
          : 'span',
      }),
      0,
    ];
  },
});

export const HiddenComments = Extension.create({
  name: 'hiddenComments',
  addGlobalAttributes() {
    return [
      {
        types: ['paragraph', 'heading'],
        attributes: {
          comments: { default: [] as string[], rendered: false },
        },
      },
    ];
  },
});

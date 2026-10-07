// Which editorial-mark commands (!! / -- / ++) apply to a selection.
//
//  - no (non-blank) text selected, or selection spans paragraphs, or overlaps
//    a span other than as that span's exact text  -> none enabled
//  - selection is exactly the text of an editorial span -> only that kind is
//    enabled, and it REMOVES the mark
//  - otherwise -> all three enabled; each ADDS its mark

import type { Mark, Node as PMNode } from '@tiptap/pm/model';
import type { EditorialKind } from './schema';

export type EditorialAvailability =
  | { mode: 'none' }
  | { mode: 'add' }
  | { mode: 'remove'; kind: EditorialKind; mark: Mark; from: number; to: number };

/** The contiguous run of text carrying exactly `mark` that contains position `pos`. */
function markRunAround(doc: PMNode, pos: number, mark: Mark): { from: number; to: number } | null {
  const $pos = doc.resolve(pos);
  const parent = $pos.parent;
  const base = $pos.start();
  const runs: { from: number; to: number }[] = [];
  let cur: { from: number; to: number } | null = null;
  parent.forEach((child, offset) => {
    const start = base + offset;
    if (mark.isInSet(child.marks)) {
      if (cur && cur.to === start) cur.to = start + child.nodeSize;
      else runs.push((cur = { from: start, to: start + child.nodeSize }));
    } else {
      cur = null;
    }
  });
  return runs.find((r) => r.from <= pos && pos < r.to) ?? null;
}

export function editorialAvailability(doc: PMNode, from: number, to: number): EditorialAvailability {
  if (from === to || !doc.textBetween(from, to, ' ').trim()) return { mode: 'none' };
  const $from = doc.resolve(from);
  if (!$from.sameParent(doc.resolve(to))) return { mode: 'none' };

  // The DOCUMENT's schema, not the standalone one: the live tiptap editor builds
  // its own (name-identical) schema, and MarkTypes compare by identity.
  const spanType = doc.type.schema.marks.span;
  if (!doc.rangeHasMark(from, to, spanType)) return { mode: 'add' };

  // Overlaps a span: only an exact editorial span selection is actionable.
  const first = doc.resolve(from + 1).parent.childAfter(from + 1 - $from.start()).node;
  const mark = first?.marks.find((m) => m.type === spanType);
  const kind = mark?.attrs.kind as EditorialKind | '' | undefined;
  if (!mark || !kind) return { mode: 'none' };
  const run = markRunAround(doc, from + 1, mark);
  if (!run || run.from !== from || run.to !== to) return { mode: 'none' };
  return { mode: 'remove', kind, mark, from, to };
}

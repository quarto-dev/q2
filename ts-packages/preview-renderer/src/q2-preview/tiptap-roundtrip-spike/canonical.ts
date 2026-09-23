// THROWAWAY SPIKE (bd-sjb4pzx8) — canonicalize a Pandoc AST for semantic comparison.
// The round-trip bar is "re-parses to the same AST", not byte-identity, so we strip
// source info and (optionally) normalize whitespace the way the writer legitimately may.
// Safe to delete.

import type { AstNode } from './pampa';

// Source-tracking / volatile fields that must not affect equivalence.
const STRIP_KEYS = new Set(['s', 'a', 'l', 'targetS', 'textS']);
const VOLATILE_CITE_KEYS = new Set(['citationHash', 'citationNoteNum', 'citationIdS']);

function stripObj(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(stripObj);
  if (value && typeof value === 'object') {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      if (STRIP_KEYS.has(k) || VOLATILE_CITE_KEYS.has(k)) continue;
      out[k] = stripObj(v);
    }
    return out;
  }
  return value;
}

/** Merge Space/SoftBreak into spaces and coalesce adjacent Str runs, recursively. */
function normalizeInlines(items: AstNode[]): AstNode[] {
  const spaced = items.map((node) => {
    if (node.t === 'Space' || node.t === 'SoftBreak') return { t: 'Str', c: ' ' } as AstNode;
    if (Array.isArray(node.c)) {
      // Recurse into inline containers (Emph/Strong/Link label/etc.).
      const c = node.c as unknown[];
      const looksInlineList = c.length > 0 && c.every((x) => x && typeof x === 'object' && 't' in (x as object));
      if (looksInlineList) return { ...node, c: normalizeInlines(c as AstNode[]) };
      // Link: c = [attr, inlines, target]
      if (node.t === 'Link' || node.t === 'Image') {
        const [attr, label, target] = node.c as [unknown, AstNode[], unknown];
        return { ...node, c: [attr, normalizeInlines(label), target] };
      }
    }
    return node;
  });
  // Coalesce adjacent Str.
  const merged: AstNode[] = [];
  for (const node of spaced) {
    const last = merged[merged.length - 1];
    if (node.t === 'Str' && last && last.t === 'Str') {
      last.c = (last.c as string) + (node.c as string);
    } else {
      merged.push({ ...node });
    }
  }
  // Collapse runs of spaces inside Str and trim the list edges.
  for (const node of merged) {
    if (node.t === 'Str') node.c = (node.c as string).replace(/\s+/g, ' ');
  }
  return merged;
}

function normalizeBlocks(items: AstNode[]): AstNode[] {
  return items.map((node) => {
    const out = { ...node };
    if ((node.t === 'Para' || node.t === 'Plain' || node.t === 'Header') && Array.isArray(node.c)) {
      if (node.t === 'Header') {
        const [lvl, attr, inl] = node.c as [number, unknown, AstNode[]];
        out.c = [lvl, attr, normalizeInlines(inl)];
      } else {
        out.c = normalizeInlines(node.c as AstNode[]);
      }
    } else if (node.t === 'BlockQuote' || node.t === 'Div' || node.t === 'Figure') {
      const inner = node.t === 'Div' ? (node.c as [unknown, AstNode[]])[1] : (node.c as AstNode[]);
      const norm = normalizeBlocks(inner);
      out.c = node.t === 'Div' ? [(node.c as [unknown, AstNode[]])[0], norm] : norm;
    } else if (node.t === 'BulletList') {
      out.c = (node.c as AstNode[][]).map(normalizeBlocks);
    } else if (node.t === 'OrderedList') {
      const [attrs, its] = node.c as [unknown, AstNode[][]];
      out.c = [attrs, its.map(normalizeBlocks)];
    }
    return out;
  });
}

/** Canonical, comparable form of a document's blocks. */
export function canonical(blocks: AstNode[], opts: { normalize: boolean }): unknown {
  const stripped = stripObj(blocks) as AstNode[];
  return opts.normalize ? normalizeBlocks(stripped) : stripped;
}

export function deepEqual(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

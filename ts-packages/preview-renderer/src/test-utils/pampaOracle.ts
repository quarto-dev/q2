// Test-only oracle (bd-sjb4pzx8): parse qmd into the source-tracked untransformed
// Pandoc AST via the native `pampa` binary, and compare two ASTs semantically
// (source-info-stripped, optionally whitespace-normalized). Used by rich-text
// round-trip fidelity tests. Skips gracefully when the binary can't be built
// (e.g. a Rust-less CI runner) — see `pampaAvailable`.

import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = fileURLToPath(new URL('.', import.meta.url));
// .../ts-packages/preview-renderer/src/test-utils/ -> repo root
const REPO_ROOT = resolve(HERE, '../../../..');

export interface PoolEntry {
  r: [number, number];
  t: number;
  d: unknown;
}
export interface AstNode {
  t: string;
  c?: unknown;
  s?: number;
  [k: string]: unknown;
}
export interface RustQmdJson {
  'pandoc-api-version': [number, number, number];
  meta: Record<string, unknown>;
  blocks: AstNode[];
  astContext: { p: PoolEntry[]; files: unknown[] };
}

let binaryProbe: string | null | undefined;

/** Build (once) and return the native pampa binary path, or null if unavailable. */
function pampaBinary(): string | null {
  if (binaryProbe !== undefined) return binaryProbe;
  try {
    execFileSync('cargo', ['build', '--quiet', '--bin', 'pampa'], {
      cwd: REPO_ROOT,
      stdio: ['ignore', 'ignore', 'inherit'],
      timeout: 600_000,
    });
    binaryProbe = join(REPO_ROOT, 'target', 'debug', 'pampa');
  } catch {
    binaryProbe = null;
  }
  return binaryProbe;
}

/** True when round-trip fidelity tests can run (native pampa is buildable). */
export function pampaAvailable(): boolean {
  return pampaBinary() !== null;
}

/** Parse qmd into the source-tracked, untransformed Pandoc AST. */
export function parseUntransformed(qmd: string): RustQmdJson {
  const bin = pampaBinary();
  if (!bin) throw new Error('pampa binary unavailable');
  const dir = mkdtempSync(join(tmpdir(), 'pampa-oracle-'));
  try {
    const f = join(dir, 'in.qmd');
    writeFileSync(f, qmd, 'utf8');
    const out = execFileSync(bin, ['-t', 'json', '--json-source-location', 'full', f], {
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    });
    return JSON.parse(out) as RustQmdJson;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

// ---- semantic comparison ---------------------------------------------------

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

/**
 * Flatten nested inline marks into a per-run mark SET, matching ProseMirror's
 * flat-mark model. This makes `[**x**](u)` (Link>Strong) and `**[x](u)**`
 * (Strong>Link) compare equal — both are the run "x" carrying marks {Strong,
 * Link(u)} — while still detecting a genuinely dropped mark, word, or node
 * (the run's mark set or text would differ). Whitespace is collapsed; adjacent
 * runs with identical mark sets are merged.
 */
type InlineToken =
  | { k: 'T'; s: string; m: string[] }
  | { k: 'BR'; m: string[] }
  | { k: 'Code'; s: string; m: string[] }
  | { k: 'Leaf'; t: string; j: string; m: string[] };

function inlineTokens(items: AstNode[], marks: string[] = []): InlineToken[] {
  const out: InlineToken[] = [];
  const sorted = () => [...marks].sort();
  for (const node of items) {
    switch (node.t) {
      case 'Emph':
      case 'Strong':
      case 'Strikeout':
      case 'Underline':
      case 'SmallCaps':
      case 'Superscript':
      case 'Subscript':
        out.push(...inlineTokens(node.c as AstNode[], [...marks, node.t]));
        break;
      case 'Quoted': {
        const inner = (node.c as [unknown, AstNode[]])[1] ?? [];
        out.push(...inlineTokens(inner, [...marks, 'Quoted']));
        break;
      }
      case 'Link':
      case 'Image': {
        const [, label, target] = node.c as [unknown, AstNode[], [string, string]];
        out.push(...inlineTokens(label, [...marks, `${node.t}(${target?.[0] ?? ''})`]));
        break;
      }
      case 'Space':
      case 'SoftBreak':
        out.push({ k: 'T', s: ' ', m: sorted() });
        break;
      case 'Str':
        out.push({ k: 'T', s: node.c as string, m: sorted() });
        break;
      case 'LineBreak':
        out.push({ k: 'BR', m: sorted() });
        break;
      case 'Code':
        out.push({ k: 'Code', s: (node.c as [unknown, string])[1], m: sorted() });
        break;
      default:
        // Opaque leaf (Math/Cite/Span/RawInline/Note/...). Compared by content.
        out.push({ k: 'Leaf', t: node.t, j: JSON.stringify(node.c ?? null), m: sorted() });
        break;
    }
  }
  // Merge adjacent text runs with identical mark sets, then collapse whitespace.
  const merged: InlineToken[] = [];
  for (const tok of out) {
    const last = merged[merged.length - 1];
    if (tok.k === 'T' && last && last.k === 'T' && JSON.stringify(last.m) === JSON.stringify(tok.m)) {
      last.s += tok.s;
    } else {
      merged.push({ ...tok });
    }
  }
  for (const tok of merged) if (tok.k === 'T') tok.s = tok.s.replace(/\s+/g, ' ');
  return merged;
}

function normalizeBlocks(items: AstNode[]): AstNode[] {
  return items.map((node) => {
    const out = { ...node };
    if ((node.t === 'Para' || node.t === 'Plain') && Array.isArray(node.c)) {
      // Compare Para and Plain as the same container (tight-list items are Plain,
      // a standalone paragraph is Para; they differ only in list tightness, which
      // is captured structurally elsewhere).
      out.t = 'Para';
      out.c = inlineTokens(node.c as AstNode[]) as unknown as AstNode[];
    } else if (node.t === 'Header') {
      const [lvl, attr, inl] = node.c as [number, unknown, AstNode[]];
      out.c = [lvl, attr, inlineTokens(inl)] as unknown as AstNode[];
    } else if (node.t === 'BlockQuote' || node.t === 'Figure') {
      out.c = normalizeBlocks(node.c as AstNode[]);
    } else if (node.t === 'Div') {
      const [attr, inner] = node.c as [unknown, AstNode[]];
      out.c = [attr, normalizeBlocks(inner)];
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

export function astEqual(a: AstNode[], b: AstNode[], opts: { normalize: boolean }): boolean {
  return JSON.stringify(canonical(a, opts)) === JSON.stringify(canonical(b, opts));
}

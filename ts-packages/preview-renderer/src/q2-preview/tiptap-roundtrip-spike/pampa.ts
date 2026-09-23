// THROWAWAY SPIKE (bd-sjb4pzx8) — native-pampa oracle + AST helpers.
// Shells out to the native `pampa` binary to get the source-tracked, untransformed
// Pandoc AST (the same shape the iframe receives via WASM `parse_qmd_content`).
// No WASM init needed. Safe to delete.

import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = fileURLToPath(new URL('.', import.meta.url));
// .../ts-packages/preview-renderer/src/q2-preview/tiptap-roundtrip-spike/ -> repo root
export const REPO_ROOT = resolve(HERE, '../../../../..');

/** A source-info pool entry: byte range [start,end), target id, depth. */
export interface PoolEntry {
  r: [number, number];
  t: number;
  d: unknown;
}

export interface RustQmdJson {
  'pandoc-api-version': [number, number, number];
  meta: Record<string, unknown>;
  blocks: AstNode[];
  astContext: { p: PoolEntry[]; files: unknown[] };
}

/** Loose Pandoc AST node (annotated): `t` tag, `c` content, `s` pool index, etc. */
export interface AstNode {
  t: string;
  c?: unknown;
  s?: number;
  a?: unknown;
  l?: unknown;
  targetS?: unknown;
  textS?: unknown;
  [k: string]: unknown;
}

let cachedBinary: string | null = null;

function pampaBinary(): string {
  if (cachedBinary) return cachedBinary;
  // Build once (fast no-op if already built), then call the binary directly so
  // each fixture parse doesn't pay cargo's per-invocation overhead.
  execFileSync('cargo', ['build', '--quiet', '--bin', 'pampa'], {
    cwd: REPO_ROOT,
    stdio: ['ignore', 'inherit', 'inherit'],
  });
  cachedBinary = join(REPO_ROOT, 'target', 'debug', 'pampa');
  return cachedBinary;
}

/** Parse qmd text into the source-tracked, untransformed Pandoc AST. */
export function parseUntransformed(qmd: string): RustQmdJson {
  const bin = pampaBinary();
  const dir = mkdtempSync(join(tmpdir(), 'pampa-spike-'));
  try {
    const f = join(dir, 'in.qmd');
    writeFileSync(f, qmd, 'utf8');
    const out = execFileSync(
      bin,
      ['-t', 'json', '--json-source-location', 'full', f],
      { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 },
    );
    return JSON.parse(out) as RustQmdJson;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/** Slice the original source by UTF-8 byte offsets (matches pampa's range space). */
const encoder = new TextEncoder();
const decoder = new TextDecoder();
export function sliceBytes(src: string, start: number, end: number): string {
  return decoder.decode(encoder.encode(src).subarray(start, end));
}

/** Verbatim source text for a node, via its pool entry. Null if no source range. */
export function nodeSource(node: AstNode, pool: PoolEntry[], src: string): string | null {
  if (node.s == null) return null;
  const entry = pool[node.s];
  if (!entry) return null;
  return sliceBytes(src, entry.r[0], entry.r[1]);
}

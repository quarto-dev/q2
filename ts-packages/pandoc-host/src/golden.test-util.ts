import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import type { PandocRequest, ShareTree } from './types.ts';

export const REPO = fileURLToPath(new URL('../../../', import.meta.url));
export const readRepo = (rel: string) => readFileSync(REPO + rel);
export const readRepoJson = (rel: string) => JSON.parse(readRepo(rel).toString('utf8'));

type WireFile = { path: string; bytes: string };
const b64 = (s: string) => new Uint8Array(Buffer.from(s, 'base64'));
const files = (l: WireFile[]) => l.map((f) => ({ path: f.path, bytes: b64(f.bytes) }));

/** The golden request (bytes base64 in JSON) as the wire object (Uint8Array). */
export function goldenRequest(): PandocRequest {
  const g = readRepoJson('crates/quarto-core/schemas/pandoc-request.golden.json');
  return { ...g, files: files(g.files), resource_refs: files(g.resource_refs) };
}

export function emptyShareTree(version: string): ShareTree {
  return { share_tree_version: version, files: [] };
}

export const bytes = (s: string) => new TextEncoder().encode(s);

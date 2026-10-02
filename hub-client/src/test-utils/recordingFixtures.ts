/**
 * R0's recorded documents (`crates/quarto-core/tests/fixtures/pandoc-recordings`) as inputs for
 * the pandoc.wasm parity net (pandoc-host H3), for the formats the P7 goldens do not cover:
 * typst, pptx and epub. Each recording keeps its document directory under
 * `fs/__q2_doc__/` and native pandoc's output under `reference/`. Node-only.
 *
 * The document is seeded into the VFS at `/__q2_doc__`, the directory the reference was
 * produced in, so paths that reach the output (typst image paths, docx descriptions) are the
 * same on both sides; the replay directory in the references is mapped by
 * `relocateReferenceText`.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import { RECORDINGS, relocateReferenceText } from './pandocRecordings';

export const DOC_DIR = '/__q2_doc__';
export const RECORDED_FORMATS = ['typst', 'pptx', 'epub'] as const;
export type RecordedFormat = (typeof RECORDED_FORMATS)[number];

export interface RecordedDoc {
  name: string;
  format: RecordedFormat;
  /** Document-relative path of the `.qmd`. */
  qmd: string;
  files: { path: string; bytes: Uint8Array }[];
  referencePath: string;
  /** File extension of the output (`typ`, `pptx`, `epub`). */
  outputExt: string;
}

function walk(dir: string, rel = ''): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = path.join(dir, n);
    return statSync(p).isDirectory() ? walk(p, `${rel}${n}/`) : [`${rel}${n}`];
  });
}

export function recordedDocs(): RecordedDoc[] {
  const docs: RecordedDoc[] = [];
  for (const name of readdirSync(RECORDINGS).sort()) {
    const m = /^(.+)-(typst|pptx|epub)$/.exec(name);
    if (!m) continue;
    const dir = path.join(RECORDINGS, name);
    const docDir = path.join(dir, 'fs/__q2_doc__');
    const rels = walk(docDir);
    const qmds = rels.filter((r) => r.endsWith('.qmd') && !r.includes('/'));
    if (qmds.length !== 1) throw new Error(`${name}: expected exactly one top-level .qmd, found ${qmds.join(',')}`);
    const meta = JSON.parse(readFileSync(path.join(dir, 'meta.json'), 'utf8'));
    docs.push({
      name,
      format: m[2] as RecordedFormat,
      qmd: qmds[0],
      files: rels.map((r) => {
        const b = readFileSync(path.join(docDir, r));
        return { path: r, bytes: new Uint8Array(b.buffer, b.byteOffset, b.byteLength).slice() };
      }),
      referencePath: path.join(dir, meta.reference),
      outputExt: path.extname(meta.reference).slice(1),
    });
  }
  return docs;
}

/** The reference's extraction text, with the native replay directory mapped into request layout. */
export function referenceText(doc: RecordedDoc, extractText: (file: string) => string): string {
  return relocateReferenceText(extractText(doc.referencePath), doc.name);
}

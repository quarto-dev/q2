/**
 * Reads the text of the doc the hub's index points each path at, through a raw `Repo` on the hub's `/ws` (the way
 * `registry-dump.ts` reads the sidecar). The observer compares it with the hub's disk (DM8).
 */
import * as fs from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import type { DocHandle, Repo } from '@automerge/automerge-repo';
import type { IndexDocument, TextDocumentContent } from '@quarto/quarto-automerge-schema';
import { findDoc, makeRepo } from '../../src/core/repo';
import { ensureWasm } from '../../src/core/wasm';

export interface DocReader {
  /** The text of the doc the index names for each of `paths` (a path the index does not list is left out). */
  pointerTexts(paths: string[]): Promise<Record<string, string>>;
  close(): Promise<void>;
}

export async function connectDocReader(wsUrl: string, indexDocId: string): Promise<DocReader> {
  await ensureWasm();
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'ge19-reader-'));
  const repo: Repo = makeRepo(root, wsUrl);
  const index = await findDoc<IndexDocument>(repo, indexDocId);
  const handles = new Map<string, DocHandle<TextDocumentContent>>();
  return {
    async pointerTexts(paths) {
      const files = (index.doc() as IndexDocument).files;
      const out: Record<string, string> = {};
      for (const p of paths) {
        const id = files[p];
        if (!id) continue;
        let h = handles.get(id);
        if (!h) {
          h = await findDoc<TextDocumentContent>(repo, id);
          handles.set(id, h);
        }
        out[p] = String((h.doc() as TextDocumentContent).text ?? '');
      }
      return out;
    },
    async close() {
      await repo.shutdown().catch(() => {});
      await fs.rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }).catch(() => {});
    },
  };
}

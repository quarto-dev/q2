/**
 * GE-14: read the registry sidecar of a running hub through a raw `Repo`, the way Gate 0 item 1 proved: connect to the
 * hub's `/ws`, read the sidecar's doc id from the index's `epochs` key, fetch the doc, and decode it with the
 * TypeScript registry. `bytes` is `A.save(doc)`, which a caller can write to a fixture file.
 */
import * as A from '@automerge/automerge/slim';
import * as fs from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import { asString, readEntries, readRetired, type Entry, type Sidecar } from '@quarto/quarto-git-epochs';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';
import { findDoc, makeRepo } from '../../src/core/repo';
import { ensureWasm } from '../../src/core/wasm';

export interface SidecarDump {
  sidecarId: string;
  entries: Entry[];
  retired: Entry[];
  bytes: Uint8Array;
}

export async function dumpSidecar(wsUrl: string, indexDocId: string): Promise<SidecarDump> {
  await ensureWasm();
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'ge14-dump-'));
  const repo = makeRepo(root, wsUrl);
  try {
    const index = await findDoc<IndexDocument>(repo, indexDocId);
    const key = (index.doc() as { epochs?: unknown } | undefined)?.epochs;
    if (key == null) throw new Error('the index has no `epochs` key: the hub is not tracking git');
    const sidecarId = asString(key);
    const sidecar = await findDoc<Sidecar>(repo, sidecarId);
    const doc = sidecar.doc() as A.Doc<Sidecar>;
    return { sidecarId, entries: readEntries(doc), retired: readRetired(doc), bytes: A.save(doc) };
  } finally {
    await repo.shutdown().catch(() => {});
    await fs.rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }).catch(() => {});
  }
}

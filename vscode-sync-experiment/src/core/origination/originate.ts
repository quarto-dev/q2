/**
 * "Share this folder": originate a project from a folder (GE-12, G24). The order matters and is the point:
 *
 * 1. bind the folder (`project.json` with the owner id) so an interruption leaves a resumable folder;
 * 2. create every doc, the index, the sidecar, and the registry entries offline, in the folder's own storage;
 * 3. connect and sync;
 * 4. wait until the server holds the heads of every doc.
 *
 * Only then does the caller show the link. The repo is shut down at the end either way, so the normal sync (which also
 * finishes an interrupted upload) starts from local storage. No `vscode` import.
 */
import { createHash, randomUUID } from 'node:crypto';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import * as A from '@automerge/automerge';
import type { DocHandle, Repo } from '@automerge/automerge-repo/slim';
import { createIndexDocumentContent, inferMimeType, setIdentity, type IndexDocument } from '@quarto/quarto-automerge-schema';
import { registryKey, type Sidecar } from '@quarto/quarto-git-epochs';
import { excludeMetaDir } from '../git-exclude';
import { DEFAULT_SYNC_SERVER, writeProjectMeta, type ProjectMeta } from '../meta';
import type { Identity } from '../presence';
import { makeRepo, webSocketAdapter } from '../repo';
import { META_DIR } from '../ignore';
import type { Selection } from './select';
import { waitForServerHeads } from './wait-heads';

/** G32 / GE-12: how long step 4 waits for the server before the publish is reported as unfinished. */
export const PUBLISH_WAIT_MS = 30_000;

export interface OriginateInput {
  dir: string;
  selection: Selection;
  server: string;
  identity: Identity;
  /** Default: a fresh random id. */
  ownerId?: string;
}

export interface Built {
  repo: Repo;
  index: DocHandle<IndexDocument>;
  /** Null for a plain folder. */
  sidecar: DocHandle<Sidecar> | null;
  /** Every doc created, for the heads wait. */
  handles: DocHandle<unknown>[];
  meta: ProjectMeta & { ownerId: string };
}

export interface OriginateDeps {
  /** Go online (default: add the websocket adapter). The recording fake network of the tests replaces it. */
  connect?(built: Built, server: string): void;
  waitMs?: number;
}

export type OriginateResult =
  | { ok: true; indexDocId: string; meta: ProjectMeta; handles: number }
  | { ok: false; reason: 'timeout'; pending: string[]; indexDocId: string; meta: ProjectMeta };

const int = (n: number) => new A.Int(n) as unknown as number;

/** The project name: the `_quarto.yml` title, as `projectFolderName` takes it, else the folder name. */
export async function shareName(dir: string, published: string[]): Promise<string> {
  let title: string | undefined;
  if (published.includes('_quarto.yml')) {
    const yml = await fs.readFile(path.join(dir, '_quarto.yml'), 'utf8').catch(() => '');
    title = /^\s*title:\s*["']?(.+?)["']?\s*$/m.exec(yml)?.[1];
  }
  return title?.trim() || path.basename(dir);
}

/** `index.qmd` when published, else the first `.qmd`, else the first file. */
export function shareFile(published: string[]): string | undefined {
  const sorted = [...published].sort((a, b) => a.localeCompare(b));
  return sorted.find((p) => p === 'index.qmd') ?? sorted.find((p) => p.endsWith('.qmd')) ?? sorted[0];
}

/** Steps 1 and 2: bind the folder and create everything offline. */
export async function buildOffline(input: OriginateInput): Promise<Built> {
  const { dir, selection, identity } = input;
  const git = selection.kind === 'git';
  const ownerId = input.ownerId ?? randomUUID();
  const published = selection.files.map((f) => f.path);
  await fs.mkdir(path.join(dir, META_DIR), { recursive: true });
  if (git) await excludeMetaDir(dir).catch(() => false);
  const repo = makeRepo(dir, null);
  const index = repo.create<IndexDocument>(createIndexDocumentContent());

  // 1. Bind first: from here on an interrupted publish is a bound folder whose local storage holds the project.
  const name = await shareName(dir, published);
  const file = shareFile(published);
  const meta: ProjectMeta & { ownerId: string } = {
    indexDocId: index.documentId,
    syncServer: input.server || DEFAULT_SYNC_SERVER,
    createdAt: new Date().toISOString(),
    mode: git ? 'clone' : 'mirror',
    ownerId,
    name,
    ...(file ? { file } : {}),
    ...(git ? { repo: { roots: selection.roots, prefix: selection.prefix } } : {}),
  };
  await writeProjectMeta(dir, meta);

  // 2. Every doc, then the sidecar with all its entries in its first change, then the index in one change.
  const handles: DocHandle<unknown>[] = [index as DocHandle<unknown>];
  const files: Record<string, string> = {};
  const entries: Record<string, number> = {};
  for (const f of selection.files) {
    const bytes = await fs.readFile(path.join(dir, f.path));
    const handle = f.binary
      ? repo.create<unknown>({ content: new Uint8Array(bytes), mimeType: inferMimeType(f.path), hash: createHash('sha256').update(bytes).digest('hex') })
      : repo.create<unknown>({ text: bytes.toString('utf8') });
    handles.push(handle as DocHandle<unknown>);
    files[f.path] = handle.documentId;
    if (git && f.blob) entries[registryKey(f.path, f.blob, handle.documentId)] = int(0); // the HEAD blob, even when the file is dirty
  }
  let sidecar: DocHandle<Sidecar> | null = null;
  if (git) {
    // The maps are created in the first change by this one writer (contract: "Creating the maps"); the entries follow in one change.
    sidecar = repo.create<Sidecar>({ version: 1, entries: {}, retired: {} } as Sidecar);
    sidecar.change((d) => {
      for (const [k, gen] of Object.entries(entries)) d.entries[k] = gen;
    });
    handles.push(sidecar as DocHandle<unknown>);
  }
  index.change((d) => {
    for (const [p, id] of Object.entries(files)) d.files[p] = id;
    setIdentity(d, identity.userId, identity.userName, identity.userColor);
    (d as { owner?: string }).owner = ownerId;
    if (git) {
      (d as { epochs?: string }).epochs = sidecar!.documentId;
      (d as { repo?: unknown }).repo = { roots: selection.roots, prefix: selection.prefix };
    }
  });
  await repo.flush();
  return { repo, index, sidecar, handles, meta };
}

/** Steps 1 to 4. Resolves `ok` only after the server confirmed the heads of every doc. */
export async function originate(input: OriginateInput, deps: OriginateDeps = {}): Promise<OriginateResult> {
  const built = await buildOffline(input);
  try {
    // 3. Connect.
    const connect = deps.connect ?? ((b: Built, server: string) => b.repo.networkSubsystem.addNetworkAdapter(webSocketAdapter(server)));
    connect(built, built.meta.syncServer);
    // 4. Wait for the server's heads.
    const wait = await waitForServerHeads(built.repo, built.handles, deps.waitMs ?? PUBLISH_WAIT_MS);
    const indexDocId = built.index.documentId;
    if (!wait.ok) return { ok: false, reason: 'timeout', pending: wait.pending, indexDocId, meta: built.meta };
    return { ok: true, indexDocId, meta: built.meta, handles: built.handles.length };
  } finally {
    await built.repo.flush().catch(() => {});
    await built.repo.shutdown().catch(() => {});
  }
}
